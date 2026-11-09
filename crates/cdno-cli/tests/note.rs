//! End-to-end tests for `cdno note new` / `cdno note list` — the generic
//! create/list command for config-defined custom note types. Runs the built
//! binary via `assert_cmd` so clap dispatch and the domain create path are both
//! exercised.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn cdno() -> Command {
    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd
}

/// Init a vault and register a `person` custom type in its config.
fn init_person_vault(dir: &Path) {
    cdno().arg("init").arg(dir).assert().success();
    let cfg = dir.join(".cuaderno/config.toml");
    let mut content = fs::read_to_string(&cfg).unwrap_or_default();
    content.push_str(
        "\n[note_types.person]\nfolder = \"people\"\nrequired = [\"name\"]\noptional = [\"role\"]\n",
    );
    fs::write(&cfg, content).unwrap();
}

fn vault_arg(dir: &Path) -> String {
    dir.to_str().unwrap().to_owned()
}

#[test]
fn note_create_creates_and_note_list_finds_it() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "note",
            "create",
            "person",
            "--title",
            "Ada Lovelace",
            "--field",
            "name=Ada",
            "--field",
            "role=advisor",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("people/ada-lovelace.md"));

    let created = dir.path().join("people/ada-lovelace.md");
    assert!(created.exists());
    let content = fs::read_to_string(&created).unwrap();
    assert!(content.contains("type: person"), "{content}");
    assert!(content.contains("name: Ada"), "{content}");

    // The created note lints clean.
    cdno().args(["--vault", &v, "lint"]).assert().success();

    cdno()
        .args(["--vault", &v, "note", "list", "person"])
        .assert()
        .success()
        .stdout(predicate::str::contains("people/ada-lovelace.md"));
}

#[test]
fn note_create_json_emits_the_write_result() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault", &v, "--json", "note", "create", "person", "--title", "Ada", "--field",
            "name=Ada",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"path\""))
        .stdout(predicate::str::contains("people/ada.md"));
}

#[test]
fn note_create_rejects_a_missing_required_field() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault", &v, "note", "create", "person", "--title", "Nameless",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires field"));
}

#[test]
fn note_create_rejects_an_undeclared_field() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "note",
            "create",
            "person",
            "--title",
            "Ada",
            "--field",
            "name=Ada",
            "--field",
            "hobby=chess",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no field 'hobby'"));
}

#[test]
fn note_create_rejects_an_unregistered_type() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault", &v, "note", "create", "gadget", "--title", "Widget",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown note type"));
}

#[test]
fn note_list_reports_when_empty() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args(["--vault", &v, "note", "list", "person"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No `person` notes."));
}

// ---------------------------------------------------------------------
// --body-file and --origin (RFC 0002 T6, #619)
// ---------------------------------------------------------------------

/// A concept template with a `{{body}}` slot, written where the `concept`
/// type `cdno init` declares looks for it.
fn write_concept_template_with_body(dir: &Path) {
    fs::write(
        dir.join(".cuaderno/templates/concept.md"),
        "---\ntype: concept\ncreated: {{created}}\n---\n\n# {{title}}\n\n{{body}}\n",
    )
    .unwrap();
}

/// A concept template with no `{{body}}` slot, replacing the one `cdno init`
/// installs, for the paths that must not demand `--body-file`.
fn write_concept_template_without_body(dir: &Path) {
    fs::write(
        dir.join(".cuaderno/templates/concept.md"),
        "---\ntype: concept\ncreated: {{created}}\n---\n\n# {{title}}\n",
    )
    .unwrap();
}

#[test]
fn note_create_writes_body_file_and_origin() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_with_body(dir.path());
    let v = vault_arg(dir.path());
    let body_file = dir.path().join("body.md");
    fs::write(&body_file, "The inverse of a low-rank update.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
            "--body-file",
            body_file.to_str().unwrap(),
            "--origin",
            "[[journal/2026/daily/2026-09-02#Woodbury identity]]",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("concepts/woodbury-identity.md"));

    let content = fs::read_to_string(dir.path().join("concepts/woodbury-identity.md")).unwrap();
    assert!(
        content.contains("# Woodbury identity\n\nThe inverse of a low-rank update.\n"),
        "{content}"
    );
    assert!(!content.contains("{{body}}"), "{content}");
    // The template has no `{{origin}}`; the frontmatter is reconciled so the
    // promotion link is still recorded, as a string.
    assert!(
        content.contains("origin: '[[journal/2026/daily/2026-09-02#Woodbury identity]]'"),
        "{content}"
    );
}

#[test]
fn note_create_writes_origin_into_frontmatter() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_without_body(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
            "--origin",
            "[[journal/2026/daily/2026-09-02#Woodbury identity]]",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(dir.path().join("concepts/woodbury-identity.md")).unwrap();
    assert!(
        content.contains("origin: '[[journal/2026/daily/2026-09-02#Woodbury identity]]'"),
        "{content}"
    );
}

#[test]
fn note_create_without_body_file_fails_when_the_template_has_a_body_slot() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_with_body(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "missing required flag: --body-file",
        ));
    assert!(!dir.path().join("concepts/woodbury-identity.md").exists());
}

#[test]
fn note_create_without_body_file_succeeds_when_the_template_has_no_body_slot() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_without_body(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
        ])
        .assert()
        .success();
    assert!(dir.path().join("concepts/woodbury-identity.md").exists());
}

#[test]
fn note_create_with_a_body_var_does_not_demand_body_file() {
    // A `--var body=` fills `{{body}}` itself, so `--body-file` is neither
    // prompted nor demanded under `--no-interactive`.
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_with_body(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
            "--var",
            "body=from the var",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(dir.path().join("concepts/woodbury-identity.md")).unwrap();
    assert!(
        content.ends_with("# Woodbury identity\n\nfrom the var\n"),
        "{content}"
    );
}

#[test]
fn note_create_with_a_vault_variable_body_does_not_demand_body_file() {
    // A `[variables] body` in the config fills `{{body}}` too, so under
    // `--no-interactive` the create succeeds without `--body-file`.
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_with_body(dir.path());
    let cfg = dir.path().join(".cuaderno/config.toml");
    let mut config = fs::read_to_string(&cfg).unwrap();
    config.push_str("\n[variables]\nbody = \"from the vault\"\n");
    fs::write(&cfg, config).unwrap();
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(dir.path().join("concepts/woodbury-identity.md")).unwrap();
    assert!(
        content.ends_with("# Woodbury identity\n\nfrom the vault\n"),
        "{content}"
    );
}

#[test]
fn note_create_drops_a_leading_title_heading_from_the_body_file() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_with_body(dir.path());
    let v = vault_arg(dir.path());
    let body_file = dir.path().join("body.md");
    fs::write(&body_file, "# Woodbury identity\n\nThe inverse.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
            "--body-file",
            body_file.to_str().unwrap(),
        ])
        .assert()
        .success();
    let content = fs::read_to_string(dir.path().join("concepts/woodbury-identity.md")).unwrap();
    assert_eq!(
        content.matches("# Woodbury identity").count(),
        1,
        "{content}"
    );
}

#[test]
fn note_create_help_names_its_value_types_and_the_title_rule() {
    cdno()
        .args(["note", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--body-file <PATH>"))
        .stdout(predicate::str::contains("--origin <STRING>"))
        .stdout(predicate::str::contains("without the title"));
    cdno()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("`note create`"));
}

// ---------------------------------------------------------------------
// cdno note revise (RFC 0002 T11, #624)
// ---------------------------------------------------------------------

const CONCEPT_PATH: &str = "concepts/woodbury-identity.md";

/// A vault holding one concept, `concepts/woodbury-identity.md`, with a
/// `## Proof` section, created through `cdno note create`.
fn init_concept_vault(dir: &Path) {
    cdno().arg("init").arg(dir).assert().success();
    write_concept_template_with_body(dir);
    let body_file = dir.join("seed-body.md");
    fs::write(&body_file, "Cheap updates.\n\n## Proof\n\nBy expansion.\n").unwrap();
    cdno()
        .args([
            "--vault",
            &vault_arg(dir),
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
            "--body-file",
            body_file.to_str().unwrap(),
        ])
        .assert()
        .success();
}

fn read_concept(dir: &Path) -> String {
    fs::read_to_string(dir.join(CONCEPT_PATH)).unwrap()
}

/// Today's daily note, where a revision is logged. The CLI stamps the
/// local date, so the test does too.
fn read_today_daily(dir: &Path) -> String {
    let today = chrono::Local::now().date_naive();
    let path = dir.join(format!(
        "journal/{}/daily/{}.md",
        today.format("%Y"),
        today.format("%Y-%m-%d")
    ));
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

#[test]
fn note_revise_replaces_the_body_and_logs_the_revision() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    let body_file = dir.path().join("revised.md");
    fs::write(
        &body_file,
        "\n# Woodbury identity\n\nA rank-k correction.\n",
    )
    .unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "revise",
            "woodbury-identity",
            "--body-file",
            body_file.to_str().unwrap(),
            "--reason",
            "tightened the statement",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("Revised {CONCEPT_PATH}")));

    let content = read_concept(dir.path());
    assert!(content.starts_with("---\ntype: concept\n"), "{content}");
    assert!(
        content.ends_with("---\n\n# Woodbury identity\n\nA rank-k correction.\n"),
        "{content}"
    );
    assert!(!content.contains("Cheap updates."), "{content}");
    let daily = read_today_daily(dir.path());
    assert!(
        daily.contains("revised [[concepts/woodbury-identity]] — tightened the statement"),
        "{daily}"
    );
}

#[test]
fn note_revise_upserts_an_existing_section_with_an_anchored_log_line() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    let content_file = dir.path().join("proof.md");
    fs::write(&content_file, "By the push-through identity.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "revise",
            "concept:woodbury-identity",
            "--section",
            "Proof",
            "--content-file",
            content_file.to_str().unwrap(),
            "--reason",
            "shorter proof",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("Revised {CONCEPT_PATH}")));

    let content = read_concept(dir.path());
    assert!(
        content.contains("## Proof\n\nBy the push-through identity.\n"),
        "{content}"
    );
    assert!(!content.contains("By expansion."), "{content}");
    assert!(content.contains("Cheap updates."), "{content}");
    let daily = read_today_daily(dir.path());
    assert!(
        daily.contains("revised [[concepts/woodbury-identity#Proof]] — shorter proof"),
        "{daily}"
    );
}

#[test]
fn note_revise_appends_a_missing_section() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    let content_file = dir.path().join("examples.md");
    fs::write(&content_file, "Kalman gain updates.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "revise",
            CONCEPT_PATH,
            "--section",
            "Examples",
            "--content-file",
            content_file.to_str().unwrap(),
            "--reason",
            "added an example",
        ])
        .assert()
        .success();

    let content = read_concept(dir.path());
    assert!(
        content.ends_with("## Examples\n\nKalman gain updates.\n"),
        "{content}"
    );
    let daily = read_today_daily(dir.path());
    assert!(
        daily.contains("revised [[concepts/woodbury-identity#Examples]] — added an example"),
        "{daily}"
    );
}

#[test]
fn note_revise_without_reason_fails_non_interactively() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    let body_file = dir.path().join("revised.md");
    fs::write(&body_file, "New body.\n").unwrap();
    let before = read_concept(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "revise",
            "woodbury-identity",
            "--body-file",
            body_file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing required flag: --reason"));
    assert_eq!(read_concept(dir.path()), before);
}

#[test]
fn note_revise_without_a_body_source_fails_non_interactively() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "revise",
            "woodbury-identity",
            "--reason",
            "why",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "missing required flag: --body-file",
        ));
}

#[test]
fn note_revise_section_and_content_file_require_each_other_and_exclude_body_file() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    let file = dir.path().join("x.md");
    fs::write(&file, "x\n").unwrap();
    let file = file.to_str().unwrap();
    let base = ["--vault", &v, "--no-interactive", "note", "revise"];

    cdno()
        .args(base)
        .args(["woodbury-identity", "--section", "Proof", "--reason", "r"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--content-file <PATH>"));
    cdno()
        .args(base)
        .args(["woodbury-identity", "--content-file", file, "--reason", "r"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--section <STRING>"));
    cdno()
        .args(base)
        .args([
            "woodbury-identity",
            "--body-file",
            file,
            "--section",
            "Proof",
            "--content-file",
            file,
            "--reason",
            "r",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

/// The lost-update guard, end to end. The body or section file is a FIFO,
/// which the command opens only after it has read the note and kept its
/// hash, so the test's open for writing returns exactly then: the edit below
/// lands deterministically between the command's read and its commit, while
/// the command is blocked reading the file. The commit must be refused with
/// the domain's `StaleRevision` message and the concurrent edit must survive.
///
/// `source_args` builds the revision flags around the FIFO's path.
#[cfg(unix)]
fn assert_a_raced_revision_is_refused(source_args: impl Fn(&str) -> Vec<String>) {
    use std::io::Write;

    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    let fifo = dir.path().join("body.fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("running mkfifo");
    assert!(status.success(), "mkfifo failed");

    let child = std::process::Command::new(assert_cmd::cargo::cargo_bin("cdno"))
        .env_remove("CUADERNO_VAULT_PATH")
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "revise",
            "woodbury-identity",
            "--reason",
            "raced",
        ])
        .args(source_args(fifo.to_str().unwrap()))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawning cdno");

    // On a separate thread, so a command that fails before opening the FIFO
    // fails this test on its output instead of hanging it on the open.
    let note = dir.path().join(CONCEPT_PATH);
    let editor_save = read_concept(dir.path()).replace("Cheap updates.", "Saved by the editor.");
    let expected = editor_save.clone();
    std::thread::spawn(move || {
        let mut writer = fs::OpenOptions::new()
            .write(true)
            .open(&fifo)
            .expect("opening the FIFO for writing");
        fs::write(&note, editor_save).unwrap();
        writer
            .write_all(b"The revision that lost the race.\n")
            .unwrap();
    });

    let output = child.wait_with_output().expect("waiting for cdno");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "revision should be refused");
    assert!(stderr.contains("changed since it was read"), "{stderr}");
    assert_eq!(read_concept(dir.path()), expected);
    assert!(!read_today_daily(dir.path()).contains("revised [["));
}

#[cfg(unix)]
#[test]
fn note_revise_refuses_a_note_changed_after_it_was_read() {
    assert_a_raced_revision_is_refused(|fifo| vec!["--body-file".into(), fifo.into()]);
}

/// The section form passes the hash too, so the same race is refused there.
#[cfg(unix)]
#[test]
fn note_revise_refuses_a_section_revision_of_a_note_changed_after_it_was_read() {
    assert_a_raced_revision_is_refused(|fifo| {
        vec![
            "--section".into(),
            "Proof".into(),
            "--content-file".into(),
            fifo.into(),
        ]
    });
}

/// A body file that cannot be read (missing, or a directory) is named in
/// the error, and nothing is written or logged.
#[test]
fn note_revise_reports_an_unreadable_body_file_and_writes_nothing() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    let before = read_concept(dir.path());
    let missing = dir.path().join("does-not-exist.md");
    let directory = dir.path().join("a-directory");
    fs::create_dir(&directory).unwrap();

    for unreadable in [&missing, &directory] {
        let shown = unreadable.to_str().unwrap();
        cdno()
            .args([
                "--vault",
                &v,
                "--no-interactive",
                "note",
                "revise",
                "woodbury-identity",
                "--body-file",
                shown,
                "--reason",
                "r",
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains(format!(
                "reading --body-file {shown}"
            )));
        assert_eq!(read_concept(dir.path()), before, "{shown}");
        assert!(
            !read_today_daily(dir.path()).contains("revised [["),
            "{shown}"
        );
    }
}

#[test]
fn note_revise_json_emits_the_outcome_fields() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    let content_file = dir.path().join("proof.md");
    fs::write(&content_file, "Shorter.\n").unwrap();
    let args = [
        "--vault",
        &v,
        "--json",
        "note",
        "revise",
        "woodbury-identity",
        "--section",
        "Proof",
        "--content-file",
        content_file.to_str().unwrap(),
        "--reason",
        "shorter",
    ];

    let output = cdno().args(args).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["path"], CONCEPT_PATH);
    assert_eq!(json["changed"], true);
    assert_eq!(json["message"], format!("Revised {CONCEPT_PATH}"));
    assert_eq!(
        json["log_line"],
        "revised [[concepts/woodbury-identity#Proof]] — shorter"
    );
    assert_eq!(json["section_target"], "concepts/woodbury-identity#Proof");
    let first_hash = json["new_hash"].as_str().unwrap().to_owned();
    assert!(!first_hash.is_empty(), "{json}");

    // The same revision again changes nothing and logs nothing.
    let output = cdno().args(args).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["changed"], false);
    assert_eq!(json["new_hash"], first_hash.as_str());
    assert_eq!(json["log_line"], serde_json::Value::Null);
    assert_eq!(json["section_target"], serde_json::Value::Null);
    assert_eq!(json["message"], format!("No change to {CONCEPT_PATH}"));
}

#[test]
fn note_revise_refuses_a_built_in_note() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nFitting the emulator.\n";
    fs::write(dir.path().join("projects/surrogate-model.md"), project).unwrap();
    let body_file = dir.path().join("revised.md");
    fs::write(&body_file, "Overwritten.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "revise",
            "project:surrogate-model",
            "--body-file",
            body_file.to_str().unwrap(),
            "--reason",
            "should not happen",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "note 'projects/surrogate-model.md' cannot be revised: 'project' is a built-in note type",
        ));
    assert_eq!(
        fs::read_to_string(dir.path().join("projects/surrogate-model.md")).unwrap(),
        project
    );
}

#[test]
fn note_revise_reports_an_unknown_and_an_ambiguous_reference_as_open_does() {
    let dir = tempdir().unwrap();
    init_concept_vault(dir.path());
    let v = vault_arg(dir.path());
    // A stewardship and a portfolio sharing a slug, as in `tests/open.rs`.
    fs::write(
        dir.path().join("stewardships/gym.md"),
        "---\ntype: stewardship\ncontext: personal\ncreated: 2026-04-01\n---\n\n# Gym\n",
    )
    .unwrap();
    let portfolio = dir.path().join("portfolios/gym");
    fs::create_dir_all(&portfolio).unwrap();
    fs::write(
        portfolio.join("_index.md"),
        "---\ntype: portfolio\ncreated: 2026-04-01\n---\n\n# Gym\n",
    )
    .unwrap();
    let body_file = dir.path().join("revised.md");
    fs::write(&body_file, "x\n").unwrap();
    let body_file = body_file.to_str().unwrap();
    let base = ["--vault", &v, "--no-interactive", "note", "revise"];
    let tail = ["--body-file", body_file, "--reason", "r"];

    cdno()
        .args(base)
        .arg("zzzzqqqq")
        .args(tail)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "no note matching `zzzzqqqq` — `cdno open --list` shows every note",
        ));
    cdno()
        .args(base)
        .arg("woodbury")
        .args(tail)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "did you mean concept:woodbury-identity",
        ));
    cdno()
        .args(base)
        .arg("gym")
        .args(tail)
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous reference `gym`"))
        .stderr(predicate::str::contains("portfolio:gym"))
        .stderr(predicate::str::contains("stewardship:gym"));
}

#[test]
fn note_revise_help_lists_no_required_flag() {
    let output = cdno().args(["note", "revise", "--help"]).output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    // clap spells every required argument into the usage line, so a usage
    // line of only optional groups is the proof that no flag is required.
    let usage = help
        .lines()
        .find(|l| l.starts_with("Usage:"))
        .expect("a usage line");
    assert_eq!(usage, "Usage: cdno note revise [OPTIONS] [NOTE]", "{help}");
    for flag in [
        "--body-file <PATH>",
        "--section <STRING>",
        "--content-file <PATH>",
        "--reason <STRING>",
    ] {
        assert!(help.contains(flag), "{flag} missing from:\n{help}");
    }
}

#[test]
fn create_with_slug_names_the_file() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args(["--vault", &v, "note", "create", "person"])
        .args(["--title", "Ada Lovelace", "--field", "name=Ada"])
        .args(["--slug", "surrogate-model"])
        .assert()
        .success()
        .stdout(predicate::str::contains("people/surrogate-model.md"));
    assert!(dir.path().join("people/surrogate-model.md").is_file());
}

#[test]
fn slug_and_field_slug_together_are_refused() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args(["--vault", &v, "note", "create", "person"])
        .args(["--title", "Ada Lovelace", "--field", "name=Ada"])
        .args(["--field", "slug=surrogate-calibration"])
        .args(["--slug", "surrogate-model"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "the slug was given both as the explicit slug and as the 'slug' field",
        ));
    let made = fs::read_dir(dir.path().join("people")).map_or(0, |d| d.count());
    assert_eq!(made, 0, "nothing created");
}

#[test]
fn slug_and_field_slug_together_are_refused_as_json() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    let out = cdno()
        .args(["--json", "--vault", &v, "note", "create", "person"])
        .args(["--title", "Ada Lovelace", "--field", "name=Ada"])
        .args(["--field", "slug=surrogate-calibration"])
        .args(["--slug", "surrogate-model"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).expect("a JSON object");
    assert_eq!(value["code"], "slug_given_twice", "{value}");
    assert_eq!(value["details"]["field"], "slug", "{value}");
    assert!(value["message"].is_string(), "{value}");
}

// ---------------------------------------------------------------------------
// The second clone (RFC 0007 T21, §10): a rename on one clone reaches another
// through git, and only the markdown travels.
// ---------------------------------------------------------------------------

mod two_clones {
    use super::*;
    use cdno_core::index::{SqliteIndex, VaultIndex};
    use cdno_core::path::VaultPath;
    use std::process::Command as Proc;

    pub(super) fn git_available() -> bool {
        Proc::new("git").arg("--version").output().is_ok()
    }

    /// Run `git` in `dir` as a throwaway identity, never the global one.
    pub(super) fn git(dir: &Path, args: &[&str]) {
        let out = git_output(dir, args);
        assert!(
            out.status.success(),
            "git {args:?} in {}: {}{}",
            dir.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// [`git`] without the success assertion, for a command expected to
    /// fail (a conflicted merge).
    pub(super) fn git_output(dir: &Path, args: &[&str]) -> std::process::Output {
        Proc::new("git")
            .current_dir(dir)
            // Nothing from the developer's machine reaches these commands:
            // no global or system config (hooks, identity, rewrites), and no
            // identity in the environment, which would beat `-c user.*`.
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_AUTHOR_NAME")
            .env_remove("GIT_AUTHOR_EMAIL")
            .env_remove("GIT_COMMITTER_NAME")
            .env_remove("GIT_COMMITTER_EMAIL")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .expect("run git")
    }

    fn cdno_at(root: &Path) -> Command {
        let mut cmd = cdno();
        cmd.env("NO_COLOR", "1")
            .arg("--vault")
            .arg(root)
            .arg("--no-interactive");
        cmd
    }

    fn snapshot_row(root: &Path, rel: &str) -> cdno_core::index::ArchivalSnapshot {
        let index = SqliteIndex::open(root.join(".cuaderno/index.db")).expect("open index");
        index
            .find_archival_snapshot(&VaultPath::new(rel).unwrap())
            .unwrap()
            .expect("the archived note has a snapshot row")
    }

    #[test]
    fn a_rename_on_one_clone_leaves_the_other_clean_and_resolving() {
        if !git_available() {
            eprintln!("skipped: `git` is not on PATH");
            return;
        }
        let tmp = tempdir().unwrap();
        let bare = tmp.path().join("remote.git");
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "--bare", "-b", "main"]);

        // Clone A: a vault with a project, a referrer and an action note
        // that links the project, pushed to the remote.
        cdno().arg("init").arg(&a).assert().success();
        fs::write(
            a.join(".gitignore"),
            ".cuaderno/index.db*\n.cuaderno/.lock\n",
        )
        .unwrap();
        cdno_at(&a)
            .args(["project", "create", "--title", "Old", "--context", "work"])
            .assert()
            .success();
        cdno_at(&a)
            .args([
                "action",
                "add",
                "--project",
                "old",
                "--title",
                "Characterise",
            ])
            .args(["--energy", "deep", "--note"])
            .assert()
            .success();
        let active = a.join("actions/characterise.md");
        let raw = fs::read_to_string(&active).unwrap();
        fs::write(&active, format!("{raw}\nCalibrated against [[old]].\n")).unwrap();
        cdno_at(&a)
            .args(["log", "read the notes on [[old]]"])
            .assert()
            .success();
        git(&a, &["init"]);
        git(&a, &["add", "-A"]);
        git(&a, &["commit", "-m", "vault"]);
        let remote = bare.to_str().unwrap();
        git(&a, &["remote", "add", "origin", remote]);
        git(&a, &["push", "-u", "origin", "main"]);

        // Clone B archives the action, so its index holds the snapshot row,
        // and pushes the archived note.
        git(tmp.path(), &["clone", remote, "b"]);
        cdno_at(&b)
            .args([
                "action",
                "complete",
                "--project",
                "old",
                "--query",
                "characterise",
            ])
            .assert()
            .success();
        git(&b, &["add", "-A"]);
        git(&b, &["commit", "-m", "archive"]);
        git(&b, &["push"]);
        let year = fs::read_dir(b.join("actions/_done"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .into_string()
            .unwrap();
        let archived = format!("actions/_done/{year}/characterise.md");
        let before = snapshot_row(&b, &archived);
        assert!(
            before.masked_hash.is_some(),
            "archival stamps the masked hash"
        );

        // Clone A takes the archived note. A holds no snapshot row (the index
        // does not travel), so this open only reconciles the pulled note; B's
        // row carries a masked hash from its own archival, so the backfill is
        // not exercised here (T16's lint tests cover it).
        git(&a, &["pull", "--no-rebase"]);
        cdno_at(&a).arg("lint").assert().success();
        cdno_at(&a)
            .args(["note", "rename", "old", "--to", "surrogate-model"])
            .assert()
            .success();
        assert!(a.join("projects/surrogate-model.md").exists());
        git(&a, &["add", "-A"]);
        git(&a, &["commit", "-m", "rename"]);
        git(&a, &["push"]);

        // Clone B receives it. Its index still holds the pre-rename row.
        git(&b, &["pull", "--no-rebase"]);
        assert!(
            fs::read_to_string(b.join(&archived))
                .unwrap()
                .contains("[[surrogate-model]]"),
            "the rename rewrote the archived note's frozen prefix"
        );
        cdno_at(&b)
            .arg("lint")
            .assert()
            .success()
            .stdout(predicate::str::contains("[error]").not());
        cdno_at(&b)
            .args(["open", "old", "--path"])
            .assert()
            .success()
            .stdout(predicate::str::contains("projects/surrogate-model.md"));
        assert_eq!(
            snapshot_row(&b, &archived),
            before,
            "the snapshot row on B is unchanged"
        );
    }
}

// ---------------------------------------------------------------------
// `cdno note rename` (RFC 0007 T24)
// ---------------------------------------------------------------------

/// A vault with one project `old` whose slug is to be changed, a daily log
/// line linking it, and a second project `taken` holding a name.
fn init_rename_vault(dir: &Path) {
    cdno().arg("init").arg(dir).assert().success();
    for title in ["Old", "Taken"] {
        cdno()
            .args(["--vault", &vault_arg(dir), "project", "create"])
            .args(["--title", title, "--context", "work"])
            .assert()
            .success();
    }
    cdno()
        .args([
            "--vault",
            &vault_arg(dir),
            "log",
            "read the notes on [[old]]",
        ])
        .assert()
        .success();
}

/// Every file of the vault but the index and the lock, by path: what "the
/// vault is byte-identical" compares.
fn vault_bytes(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, at: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if rel.starts_with(".cuaderno/index.db") || rel == ".cuaderno/.lock" {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn rename_cmd(dir: &Path) -> Command {
    let mut cmd = cdno();
    cmd.env("NO_COLOR", "1").args([
        "--vault",
        &vault_arg(dir),
        "--no-interactive",
        "note",
        "rename",
    ]);
    cmd
}

#[test]
fn dry_run_prints_the_plan_and_writes_nothing() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let before = vault_bytes(dir.path());

    rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run: nothing written."))
        .stdout(predicate::str::contains(
            "move     projects/old.md -> projects/surrogate-model.md",
        ))
        .stdout(predicate::str::contains("2 links"))
        .stdout(predicate::str::contains(
            "alias    projects/surrogate-model.md gains `old`",
        ))
        .stdout(predicate::str::contains("renamed [[surrogate-model]]"))
        .stdout(predicate::str::contains("2 references in 1 note · plan "));

    assert_eq!(vault_bytes(dir.path()), before, "a dry run writes nothing");
}

#[test]
fn a_rename_writes_the_previewed_plan() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let args = [
        "old",
        "--to",
        "surrogate-model",
        "--reason",
        "a shorter name",
    ];
    let json = |extra: &[&str]| -> serde_json::Value {
        let out = rename_cmd(dir.path())
            .args(args)
            .args(extra)
            .arg("--json")
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).unwrap()
    };
    let planned = json(&["--dry-run"]);
    assert_eq!(planned["written"], false);
    let written = json(&[]);
    assert_eq!(written["written"], true);

    // What was written is what the dry run planned: the same moves and the
    // same edits. (Not the hashes: the log line carries a minute stamp.)
    assert_eq!(written["moves"], planned["moves"]);
    assert_eq!(written["edits"], planned["edits"]);
    assert_eq!(written["log_line"], planned["log_line"]);

    assert!(!dir.path().join("projects/old.md").exists());
    let map = fs::read_to_string(dir.path().join("projects/surrogate-model.md")).unwrap();
    assert!(map.contains("aliases:") && map.contains("old"), "{map}");
    let daily = read_today_daily(dir.path());
    assert!(
        daily.contains("read the notes on [[surrogate-model]]"),
        "{daily}"
    );
    assert!(daily.contains("renamed [[surrogate-model]]"), "{daily}");
    assert!(daily.contains("was: old"), "{daily}");
    assert!(daily.contains("reason: a shorter name"), "{daily}");
}

#[test]
fn relink_previews_and_writes_like_a_rename() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model"])
        .assert()
        .success();
    // A straggler: a line written after the rename, under the old name.
    cdno()
        .args(["--vault", &vault_arg(dir.path()), "log", "back on [[old]]"])
        .assert()
        .success();
    let before = vault_bytes(dir.path());

    let preview = rename_cmd(dir.path())
        .args(["surrogate-model", "--relink", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run: nothing written."))
        .stdout(predicate::str::contains("1 link"))
        .stdout(predicate::str::contains("relinked [[surrogate-model]]"))
        .stdout(predicate::str::contains("references: 1"))
        .stdout(predicate::str::contains("1 reference in 1 note · plan "));
    let shown = String::from_utf8_lossy(&preview.get_output().stdout).into_owned();
    assert!(!shown.contains("move "), "a relink moves nothing: {shown}");
    assert!(!shown.contains("alias "), "a relink adds no alias: {shown}");
    assert_eq!(vault_bytes(dir.path()), before, "a dry run writes nothing");

    rename_cmd(dir.path())
        .args(["surrogate-model", "--relink"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Relinked 1 reference to `surrogate-model`.",
        ));
    let daily = read_today_daily(dir.path());
    assert!(daily.contains("back on [[surrogate-model]]"), "{daily}");
    assert!(!daily.contains("[[old]]"), "{daily}");
    assert!(
        daily.contains("relinked [[surrogate-model]] \u{2014} Old\n  references: 1"),
        "{daily}"
    );
    let map = fs::read_to_string(dir.path().join("projects/surrogate-model.md")).unwrap();
    assert!(map.contains("old"), "the alias stays listed: {map}");

    // Nothing left to relink.
    rename_cmd(dir.path())
        .args(["surrogate-model", "--relink"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to relink"));

    // `--relink` with `--to` or `--reason` is a usage error, and writes
    // nothing.
    let before = vault_bytes(dir.path());
    for extra in [["--to", "emulator"], ["--reason", "tidy"]] {
        rename_cmd(dir.path())
            .args(["surrogate-model", "--relink"])
            .args(extra)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("cannot be used with"));
    }
    assert_eq!(vault_bytes(dir.path()), before);

    // Neither `--to` nor `--relink`, non-interactively: the message names
    // both.
    rename_cmd(dir.path())
        .arg("surrogate-model")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "missing required flag: --to or --relink",
        ));
}

#[test]
fn json_carries_the_plan_and_written() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let out = rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model", "--json"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["dry_run"], false);
    assert_eq!(v["from"], "old");
    assert_eq!(v["new_slug"], "surrogate-model");
    assert_eq!(v["index_stale"], false);
    assert_eq!(v["references"], 2);
    assert_eq!(v["written"], true);
    assert!(v["plan_hash"].as_str().unwrap().len() > 4);
    assert_eq!(v["moves"][0]["from"], "projects/old.md");
    let kinds: Vec<&str> = v["edits"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| e["changes"].as_array().unwrap())
        .map(|c| c["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"links") && kinds.contains(&"alias") && kinds.contains(&"log_line"));
}

#[test]
fn dry_run_json_says_so_and_writes_nothing() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let before = vault_bytes(dir.path());
    let out = rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model", "--dry-run", "--json"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["written"], false);
    assert_eq!(vault_bytes(dir.path()), before);
}

#[test]
fn a_taken_slug_is_a_json_rejection() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let before = vault_bytes(dir.path());
    let out = rename_cmd(dir.path())
        .args(["old", "--to", "taken", "--json"])
        .assert()
        .code(1);
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["code"], "slug_taken");
    assert_eq!(v["details"]["kind"], "stem");
    assert_eq!(v["details"]["holder"], "projects/taken.md");
    assert_eq!(vault_bytes(dir.path()), before);
}

#[test]
fn a_note_that_cannot_be_renamed_is_a_json_rejection_with_its_reason() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let out = rename_cmd(dir.path())
        .args([&today, "--to", "elsewhere", "--json"])
        .assert()
        .code(1);
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["code"], "note_not_renamable");
    assert_eq!(v["details"]["reason"], "type daily");
}

#[test]
fn an_invalid_slug_is_refused_in_text_and_nothing_is_written() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let before = vault_bytes(dir.path());
    rename_cmd(dir.path())
        .args(["old", "--to", "Not A Slug"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slug"));
    assert_eq!(vault_bytes(dir.path()), before);
}

#[test]
fn to_is_required_without_a_terminal() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    rename_cmd(dir.path())
        .args(["old"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--to"));
}

#[test]
fn a_former_name_is_reported_on_the_preview() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model"])
        .assert()
        .success();

    rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-calibration", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("found by its former name `old`"))
        .stdout(predicate::str::contains(
            "move     projects/surrogate-model.md -> projects/surrogate-calibration.md",
        ));

    // Reached by its current name, no such line.
    rename_cmd(dir.path())
        .args([
            "surrogate-model",
            "--to",
            "surrogate-calibration",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("former name").not());
}

#[test]
fn an_incomplete_rename_lists_its_paths() {
    // The CLI cannot provoke a rollback that itself fails against real
    // files, so the domain error is built and rendered through the handler's
    // own refusal path. The domain probe (`rename_tests`) owns the cause.
    use cdno_core::error::StoreError;
    use cdno_core::path::VaultPath;
    use cdno_domain::error::DomainError;
    let failed = vec![
        VaultPath::new("projects/surrogate-model.md").unwrap(),
        VaultPath::new("journal/2026/daily/2026-11-06.md").unwrap(),
    ];
    let err = DomainError::RenameIncomplete {
        failed,
        source: StoreError::NotFound("disk gone".to_owned()),
    };
    let text = cdno_cli::commands::note::rename_refusal(err, false).to_string();
    assert!(
        text.contains("\n  projects/surrogate-model.md\n  journal/2026/daily/2026-11-06.md\n"),
        "one path per line:\n{text}"
    );
    let err = DomainError::RenameIncomplete {
        failed: vec![VaultPath::new("projects/a.md").unwrap()],
        source: StoreError::NotFound("x".to_owned()),
    };
    let object = cdno_cli::commands::rejection::rename_rejection(&err).unwrap();
    assert_eq!(object["code"], "rename_incomplete");
    assert_eq!(object["details"]["paths"][0], "projects/a.md");
}

/// `script(1)` pty driver, as `tests/project.rs` has it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_in_pty(root: &Path, args: &[&str], keys: &[&str]) -> String {
    run_in_pty_steps(
        root,
        args,
        keys.iter()
            .map(|k| PtyStep::Send((*k).to_owned()))
            .collect(),
    )
}

/// One step of a pty session.
#[cfg(any(target_os = "linux", target_os = "macos"))]
enum PtyStep {
    /// Type these keys (after a pause).
    Send(String),
    /// Wait until the command has drawn this text.
    WaitFor(&'static str),
    /// Run a closure, e.g. to change the vault from outside.
    Do(Box<dyn FnOnce()>),
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_in_pty_steps(root: &Path, args: &[&str], steps: Vec<PtyStep>) -> String {
    run_in_pty_steps_with_env(root, args, steps, &[])
}

/// [`run_in_pty_steps`] with extra environment for the command.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_in_pty_steps_with_env(
    root: &Path,
    args: &[&str],
    steps: Vec<PtyStep>,
    env: &[(&str, &Path)],
) -> String {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    let exe = assert_cmd::cargo::cargo_bin("cdno");
    // `script(1)` gives the pty no size, and a picker refuses a zero-width
    // terminal, so size it first.
    let mut inner = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        "stty cols 120 rows 40; exec \"$@\"".to_owned(),
        "sh".to_owned(),
        exe.to_str().unwrap().to_owned(),
        "--vault".to_owned(),
        root.to_str().unwrap().to_owned(),
    ];
    inner.extend(args.iter().map(|a| (*a).to_owned()));
    let mut script = Command::new("script");
    if cfg!(target_os = "macos") {
        script.args(["-q", "/dev/null"]).args(&inner);
    } else {
        let quoted: Vec<String> = inner
            .iter()
            .map(|w| format!("'{}'", w.replace('\'', "'\\''")))
            .collect();
        script.args(["-qec", &quoted.join(" "), "/dev/null"]);
    }
    script.envs(env.iter().map(|(k, v)| (*k, *v)));
    let mut child = script
        .env_remove("CUADERNO_VAULT_PATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("script(1) is available");
    let mut stdin = child.stdin.take().unwrap();
    let drawn = Arc::new(Mutex::new(Vec::new()));
    let mut stdout = child.stdout.take().unwrap();
    let sink = Arc::clone(&drawn);
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap().extend_from_slice(&buf[..n]);
        }
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    for step in steps {
        match step {
            PtyStep::Send(key) => {
                std::thread::sleep(Duration::from_millis(700));
                let _ = stdin.write_all(key.as_bytes()).and_then(|()| stdin.flush());
            }
            PtyStep::WaitFor(text) => loop {
                let seen = String::from_utf8_lossy(&drawn.lock().unwrap()).into_owned();
                if seen.contains(text) {
                    break;
                }
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("`{text}` was not drawn within 20s; it drew:\n{seen}");
                }
                std::thread::sleep(Duration::from_millis(50));
            },
            PtyStep::Do(action) => action(),
        }
    }
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            let seen = String::from_utf8_lossy(&drawn.lock().unwrap()).into_owned();
            panic!("the command did not finish within 20s; it drew:\n{seen}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(stdin);
    reader.join().unwrap();
    let out = drawn.lock().unwrap().clone();
    let raw = String::from_utf8_lossy(&out);
    let mut text = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if c != '\r' {
            text.push(c);
        }
    }
    text
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn the_picker_lists_only_renamable_notes() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let v = vault_arg(dir.path());
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    // A question with its same-slug portfolio, and an archived action.
    cdno()
        .args(["--vault", &v, "question", "create"])
        .args([
            "--domain",
            "research",
            "--text",
            "Does a surrogate beat the simulator?",
        ])
        .args(["--slug", "surrogate-accuracy"])
        .assert()
        .success();
    cdno()
        .args(["--vault", &v, "portfolio", "create"])
        .args(["--question", "surrogate-accuracy"])
        .assert()
        .success();
    cdno()
        .args([
            "--vault",
            &v,
            "action",
            "add",
            "--project",
            "old",
            "--title",
            "Characterise",
            "--energy",
            "deep",
            "--note",
        ])
        .assert()
        .success();
    cdno()
        .args([
            "--vault",
            &v,
            "action",
            "complete",
            "--project",
            "old",
            "--query",
            "characterise",
        ])
        .assert()
        .success();
    // Esc leaves the picker: it drew its rows and nothing was chosen.
    let seen = run_in_pty(dir.path(), &["note", "rename"], &["\u{1b}"]);
    assert!(seen.contains("(project)"), "projects are offered:\n{seen}");
    assert!(
        seen.contains("(question)"),
        "questions are offered:\n{seen}"
    );
    assert!(
        !seen.contains("(daily)") && !seen.contains(&today),
        "a daily note is not offered:\n{seen}"
    );
    assert!(
        !seen.contains("(portfolio)"),
        "a portfolio a question shares is not offered:\n{seen}"
    );
    assert!(
        !seen.contains("(action)"),
        "an archived action is not offered:\n{seen}"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn to_is_prompted_and_the_preview_confirms() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let before = vault_bytes(dir.path());

    // `--to` is asked for, the preview is shown, and declining the confirm
    // aborts with the vault untouched.
    let seen = run_in_pty(
        dir.path(),
        &["note", "rename", "old"],
        &["surrogate-model\r", "n\r"],
    );
    assert!(seen.contains("New slug"), "asked for --to:\n{seen}");
    assert!(
        seen.contains("move     projects/old.md -> projects/surrogate-model.md"),
        "the confirm shows the preview:\n{seen}"
    );
    assert!(seen.contains("Proceed?"), "{seen}");
    assert!(seen.contains("Aborted."), "{seen}");
    assert_eq!(
        vault_bytes(dir.path()),
        before,
        "an aborted rename writes nothing"
    );

    // Accepting writes it.
    let seen = run_in_pty(
        dir.path(),
        &["note", "rename", "old"],
        &["surrogate-model\r", "y\r"],
    );
    assert!(
        seen.contains("Renamed `old` to `surrogate-model`."),
        "{seen}"
    );
    assert!(dir.path().join("projects/surrogate-model.md").exists());
}

/// The text between the `log` line and the totals line of a preview.
fn log_block(preview: &str) -> String {
    let mut block = Vec::new();
    let mut inside = false;
    for line in preview.lines() {
        if line.starts_with("  log ") {
            inside = true;
            continue;
        }
        if inside {
            if line.starts_with("             ") {
                block.push(line.trim_start().to_owned());
            } else {
                break;
            }
        }
    }
    block.join("\n")
}

#[test]
fn the_preview_shows_the_log_line_when_no_daily_note_exists() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .args(["--vault", &vault_arg(dir.path()), "project", "create"])
        .args(["--title", "Old", "--context", "work"])
        .assert()
        .success();
    // The first command of a day: remove today's daily note.
    let today = chrono::Local::now().date_naive();
    let daily = dir.path().join(format!(
        "journal/{}/daily/{}.md",
        today.format("%Y"),
        today.format("%Y-%m-%d")
    ));
    fs::remove_file(&daily).unwrap();
    let before = vault_bytes(dir.path());

    let out = rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model", "--reason", "shorter"])
        .arg("--dry-run")
        .assert()
        .success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(
        log_block(&text),
        "renamed [[surrogate-model]] — Old\nwas: old\nreason: shorter",
        "{text}"
    );
    assert!(!text.contains("type: daily"), "no scaffold:\n{text}");
    assert_eq!(vault_bytes(dir.path()), before);
}

#[test]
fn the_preview_shows_the_log_line_under_a_hand_written_section() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    // A section below `## Logs` links the note, so the log entry is not the
    // last edit of the daily note.
    let today = chrono::Local::now().date_naive();
    let daily = dir.path().join(format!(
        "journal/{}/daily/{}.md",
        today.format("%Y"),
        today.format("%Y-%m-%d")
    ));
    let mut text = fs::read_to_string(&daily).unwrap();
    text.push_str("\n## Notes\n\nSee [[old]] for the plan.\n");
    fs::write(&daily, text).unwrap();

    let out = rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model", "--dry-run"])
        .assert()
        .success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(
        log_block(&text),
        "renamed [[surrogate-model]] — Old\nwas: old",
        "{text}"
    );
}

#[test]
fn open_reports_a_former_name_on_stderr() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model"])
        .assert()
        .success();
    let by_old = cdno()
        .args(["--vault", &vault_arg(dir.path()), "open", "old", "--path"])
        .assert()
        .success()
        .stderr(predicate::str::contains("found by its former name `old`"))
        .stdout(predicate::str::contains("projects/surrogate-model.md"));
    drop(by_old);
    cdno()
        .args(["--vault", &vault_arg(dir.path()), "open"])
        .args(["surrogate-model", "--path"])
        .assert()
        .success()
        .stderr(predicate::str::contains("former name").not());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_change_during_the_confirm_prompt_is_a_stale_plan() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let daily = {
        let today = chrono::Local::now().date_naive();
        dir.path().join(format!(
            "journal/{}/daily/{}.md",
            today.format("%Y"),
            today.format("%Y-%m-%d")
        ))
    };
    let seen = run_in_pty_steps(
        dir.path(),
        &["note", "rename", "old"],
        vec![
            PtyStep::WaitFor("New slug"),
            PtyStep::Send("surrogate-model\r".to_owned()),
            PtyStep::WaitFor("Proceed?"),
            PtyStep::Do(Box::new(move || {
                let mut text = fs::read_to_string(&daily).unwrap();
                text.push_str("\nAlso [[old]].\n");
                fs::write(&daily, text).unwrap();
            })),
            PtyStep::Send("y\r".to_owned()),
        ],
    );
    assert!(
        seen.contains("the vault changed since the rename was previewed"),
        "{seen}"
    );
    assert!(dir.path().join("projects/old.md").exists(), "nothing moved");
    assert!(!dir.path().join("projects/surrogate-model.md").exists());
}

#[test]
fn a_question_and_its_portfolio_resolve_to_the_question() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    let v = vault_arg(dir.path());
    cdno()
        .args(["--vault", &v, "question", "create", "--domain", "research"])
        .args(["--text", "Does a surrogate beat the simulator?"])
        .args(["--slug", "surrogate-accuracy"])
        .assert()
        .success();
    cdno()
        .args(["--vault", &v, "portfolio", "create"])
        .args(["--question", "surrogate-accuracy"])
        .assert()
        .success();
    // The bare slug names two notes; only the question is renamable, so it
    // is the answer rather than an ambiguity.
    rename_cmd(dir.path())
        .args(["surrogate-accuracy", "--to", "surrogate-error", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "move     questions/research/surrogate-accuracy.md -> questions/research/surrogate-error.md",
        ));
}

#[test]
fn an_auto_taken_single_hit_passes_its_path() {
    let dir = tempdir().unwrap();
    init_rename_vault(dir.path());
    // An archived action with the project's stem: the bare slug is ambiguous
    // to the resolver, but only the project is renamable, so it is taken.
    let archived = dir.path().join("actions/_done/2026");
    fs::create_dir_all(&archived).unwrap();
    fs::write(
        archived.join("old.md"),
        "---\ntype: action\nstatus: done\nproject: taken\nenergy: deep\ncreated: 2026-05-01\n---\n\n# Old step\n",
    )
    .unwrap();
    let before = vault_bytes(dir.path());

    // The domain then refuses the shared stem, with its own code.
    let out = rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model", "--json"])
        .assert()
        .code(1);
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["code"], "stem_shared");
    rename_cmd(dir.path())
        .args(["old", "--to", "surrogate-model"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("is shared with"));
    assert_eq!(vault_bytes(dir.path()), before);
}

// ---------------------------------------------------------------------
// The freshness check of `cdno note rename` (RFC 0007 §5.8 (2), T25), on
// real repositories. Each test is skipped when `git` is not on PATH.
// ---------------------------------------------------------------------

mod freshness {
    use super::two_clones::{git, git_available, git_output};
    use super::*;

    /// A rename-ready vault at `<tmp>/a`, committed, with a `.gitignore`
    /// for the index and the lock. Not yet connected to any remote.
    fn committed_vault(tmp: &Path) -> std::path::PathBuf {
        let a = tmp.join("a");
        init_rename_vault(&a);
        fs::write(
            a.join(".gitignore"),
            ".cuaderno/index.db*\n.cuaderno/.lock\n",
        )
        .unwrap();
        fs::write(a.join("notes.txt"), "base\n").unwrap();
        git(&a, &["init"]);
        git(&a, &["add", "-A"]);
        git(&a, &["commit", "-m", "vault"]);
        a
    }

    /// [`committed_vault`] pushed to a bare remote at `<tmp>/remote.git`
    /// and tracking it: up to date with its upstream.
    fn tracking_vault(tmp: &Path) -> std::path::PathBuf {
        let bare = tmp.join("remote.git");
        fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "--bare", "-b", "main"]);
        let a = committed_vault(tmp);
        git(&a, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(&a, &["push", "-u", "origin", "main"]);
        a
    }

    /// [`tracking_vault`] whose remote has one commit the clone has not
    /// fetched: another clone pushed it. Only a fetch shows `a` behind.
    fn vault_behind(tmp: &Path) -> std::path::PathBuf {
        let a = tracking_vault(tmp);
        git(
            tmp,
            &["clone", tmp.join("remote.git").to_str().unwrap(), "b"],
        );
        let b = tmp.join("b");
        fs::write(b.join("notes.txt"), "from the other clone\n").unwrap();
        git(&b, &["commit", "-am", "other clone"]);
        git(&b, &["push"]);
        a
    }

    /// `cdno note rename` with a throwaway `HOME`, so the developer's git
    /// config never reaches the git cdno runs.
    fn rename_in(tmp: &Path, vault: &Path) -> Command {
        let mut cmd = rename_cmd(vault);
        cmd.env("HOME", tmp).env("XDG_CONFIG_HOME", tmp);
        cmd
    }

    fn warnings(stderr: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(stderr)
            .lines()
            .filter(|l| l.starts_with("warning:"))
            .map(str::to_owned)
            .collect()
    }

    /// [`vault_bytes`] without `.git/`, which a fetch writes to
    /// (`FETCH_HEAD`): what "nothing written" means for the vault.
    fn tree_bytes(vault: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut bytes = vault_bytes(vault);
        bytes.retain(|path, _| !path.starts_with(".git/"));
        bytes
    }

    fn renamed(vault: &Path) -> bool {
        vault.join("projects/surrogate-model.md").exists()
            && !vault.join("projects/old.md").exists()
    }

    macro_rules! require_git {
        () => {
            if !git_available() {
                eprintln!("skipped: `git` is not on PATH");
                return;
            }
        };
    }

    #[test]
    fn behind_upstream_refuses_and_names_the_override() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = vault_behind(tmp.path());
        let before = tree_bytes(&a);
        rename_in(tmp.path(), &a)
            .args(["old", "--to", "surrogate-model"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("1 commit behind its upstream"))
            .stderr(predicate::str::contains("--skip-sync-check"))
            .stderr(predicate::str::contains("git pull"));
        assert_eq!(tree_bytes(&a), before, "nothing written");

        let out = rename_in(tmp.path(), &a)
            .args(["--json", "old", "--to", "surrogate-model"])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["code"], "behind_upstream", "{v}");
        assert_eq!(v["details"]["behind"], 1, "{v}");
        assert!(
            v["details"]["remedy"]
                .as_str()
                .unwrap()
                .contains("--skip-sync-check"),
            "{v}"
        );
        assert_eq!(tree_bytes(&a), before, "nothing written");
    }

    #[test]
    fn skip_sync_check_proceeds_when_behind() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = vault_behind(tmp.path());
        let out = rename_in(tmp.path(), &a)
            .args(["old", "--to", "surrogate-model", "--skip-sync-check"])
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(renamed(&a));
        let warned = warnings(&out.stderr);
        assert_eq!(warned.len(), 1, "{warned:?}");
        assert!(warned[0].contains("behind its upstream"), "{warned:?}");
    }

    /// [`committed_vault`] with a conflicted merge of `notes.txt` stopped
    /// mid-way: `MERGE_HEAD` is present.
    fn start_conflicted_merge(a: &Path) {
        let out = git_output(a, &["merge", "other"]);
        assert!(!out.status.success(), "the merge conflicts");
        assert!(a.join(".git/MERGE_HEAD").exists(), "the merge is paused");
    }

    /// Branch `other` and `main` both change `notes.txt`, so merging
    /// `other` into `main` conflicts.
    fn diverging_branches(a: &Path) {
        git(a, &["checkout", "-b", "other"]);
        fs::write(a.join("notes.txt"), "from other\n").unwrap();
        git(a, &["commit", "-am", "other"]);
        git(a, &["checkout", "main"]);
        fs::write(a.join("notes.txt"), "from main\n").unwrap();
        git(a, &["commit", "-am", "main"]);
    }

    #[test]
    fn a_paused_merge_refuses_even_with_skip() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = committed_vault(tmp.path());
        diverging_branches(&a);
        start_conflicted_merge(&a);
        let before = tree_bytes(&a);
        rename_in(tmp.path(), &a)
            .args(["old", "--to", "surrogate-model", "--skip-sync-check"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("a git operation is in progress"))
            .stderr(predicate::str::contains("MERGE_HEAD"));
        let out = rename_in(tmp.path(), &a)
            .args([
                "--json",
                "old",
                "--to",
                "surrogate-model",
                "--skip-sync-check",
            ])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["code"], "git_operation_in_progress", "{v}");
        assert!(
            v["details"]["reason"].as_str().unwrap().contains("merge"),
            "{v}"
        );
        assert_eq!(tree_bytes(&a), before, "nothing written");

        // The check before the lock refuses too, not only the guard under
        // it: a dry run, which never reaches the guard, is refused even
        // with the override.
        rename_in(tmp.path(), &a)
            .args([
                "old",
                "--to",
                "surrogate-model",
                "--skip-sync-check",
                "--dry-run",
            ])
            .assert()
            .failure()
            .stdout(predicate::str::contains("move").not())
            .stderr(predicate::str::contains("a git operation is in progress"));
    }

    #[test]
    fn no_upstream_warns_and_proceeds() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = committed_vault(tmp.path());
        let out = rename_in(tmp.path(), &a)
            .args(["old", "--to", "surrogate-model"])
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(renamed(&a));
        let warned = warnings(&out.stderr);
        assert_eq!(warned.len(), 1, "{warned:?}");
        assert!(warned[0].contains("no upstream"), "{warned:?}");
    }

    #[test]
    fn a_dirty_tree_does_not_refuse() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = tracking_vault(tmp.path());
        // A modified tracked note, a modified tracked file, an untracked
        // file and a staged one: none of it is a reason to refuse.
        let project = a.join("projects/taken.md");
        let text = fs::read_to_string(&project).unwrap();
        fs::write(&project, format!("{text}\nAn edit not yet committed.\n")).unwrap();
        fs::write(a.join("notes.txt"), "changed\n").unwrap();
        fs::write(a.join("scratch.txt"), "untracked\n").unwrap();
        fs::write(a.join("staged.txt"), "staged\n").unwrap();
        git(&a, &["add", "staged.txt"]);
        let out = rename_in(tmp.path(), &a)
            .args(["old", "--to", "surrogate-model"])
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(renamed(&a));
        assert_eq!(warnings(&out.stderr), Vec::<String>::new());
    }

    /// [`tracking_vault`] whose remote is then pointed at a path that does
    /// not exist: the upstream ref stays (up to date), any fetch fails.
    fn unreachable_remote(tmp: &Path) -> std::path::PathBuf {
        let a = tracking_vault(tmp);
        let gone = tmp.join("no-such-remote.git");
        git(&a, &["remote", "set-url", "origin", gone.to_str().unwrap()]);
        a
    }

    #[test]
    fn dry_run_never_fetches() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = unreachable_remote(tmp.path());
        let before = tree_bytes(&a);
        let out = rename_in(tmp.path(), &a)
            .args(["old", "--to", "surrogate-model", "--dry-run"])
            .assert()
            .success()
            .get_output()
            .clone();
        assert_eq!(
            warnings(&out.stderr),
            Vec::<String>::new(),
            "a fetch would have failed and warned"
        );
        assert_eq!(tree_bytes(&a), before);
    }

    #[test]
    fn a_failed_fetch_warns_and_proceeds() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = unreachable_remote(tmp.path());
        let out = rename_in(tmp.path(), &a)
            .args(["old", "--to", "surrogate-model"])
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(renamed(&a));
        let warned = warnings(&out.stderr);
        assert_eq!(warned.len(), 1, "{warned:?}");
        assert!(warned[0].contains("could not fetch"), "{warned:?}");
    }

    #[test]
    fn a_dry_run_refuses_as_the_write_would() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = vault_behind(tmp.path());
        // The dry run never fetches, so it is assessed against the last
        // fetch: once a fetch has shown the vault behind, it refuses.
        git(&a, &["fetch"]);
        let before = tree_bytes(&a);
        rename_in(tmp.path(), &a)
            .args(["old", "--to", "surrogate-model", "--dry-run"])
            .assert()
            .failure()
            .stdout(predicate::str::contains("Dry run").not())
            .stderr(predicate::str::contains("1 commit behind its upstream"))
            .stderr(predicate::str::contains("--skip-sync-check"));
        let out = rename_in(tmp.path(), &a)
            .args(["--json", "old", "--to", "surrogate-model", "--dry-run"])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["code"], "behind_upstream", "{v}");
        assert_eq!(tree_bytes(&a), before);
    }

    #[test]
    fn a_dry_run_with_skip_shows_the_plan_when_behind() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = vault_behind(tmp.path());
        git(&a, &["fetch"]);
        let before = tree_bytes(&a);
        let out = rename_in(tmp.path(), &a)
            .args([
                "old",
                "--to",
                "surrogate-model",
                "--dry-run",
                "--skip-sync-check",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("Dry run: nothing written."))
            .stdout(predicate::str::contains("projects/surrogate-model.md"))
            .get_output()
            .clone();
        let warned = warnings(&out.stderr);
        assert!(
            warned.len() == 1 && warned[0].contains("behind its upstream"),
            "{warned:?}"
        );
        // Under `--json` the warning is a `sync_check` entry of the plan.
        let out = rename_in(tmp.path(), &a)
            .args([
                "--json",
                "old",
                "--to",
                "surrogate-model",
                "--dry-run",
                "--skip-sync-check",
            ])
            .assert()
            .success()
            .get_output()
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let sync: Vec<_> = v["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["kind"] == "sync_check")
            .collect();
        assert_eq!(sync.len(), 1, "{v}");
        assert!(sync[0]["message"].as_str().unwrap().contains("behind"));
        assert!(
            warnings(&out.stderr).is_empty(),
            "not on stderr under --json"
        );
        assert_eq!(tree_bytes(&a), before);
    }

    #[test]
    fn json_carries_a_failed_fetch_as_a_sync_check_warning() {
        require_git!();
        let tmp = tempdir().unwrap();
        let a = unreachable_remote(tmp.path());
        let out = rename_in(tmp.path(), &a)
            .args(["--json", "old", "--to", "surrogate-model"])
            .assert()
            .success()
            .get_output()
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["written"], true, "{v}");
        let sync: Vec<_> = v["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["kind"] == "sync_check")
            .collect();
        assert_eq!(sync.len(), 1, "{v}");
        assert!(
            sync[0]["message"]
                .as_str()
                .unwrap()
                .contains("could not fetch")
        );
    }

    /// Seat C's sequence: the check before the preview finds the vault up
    /// to date; while the confirm waits, the other clone pushes and a plain
    /// `git fetch` lands in the vault; then `y`.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn fetch_during_the_confirm(extra: &[&str]) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let tmp = tempdir().unwrap();
        let a = tracking_vault(tmp.path());
        git(
            tmp.path(),
            &[
                "clone",
                tmp.path().join("remote.git").to_str().unwrap(),
                "b",
            ],
        );
        let b = tmp.path().join("b");
        let fetching = a.clone();
        let home = tmp.path().to_path_buf();
        let mut args = vec!["note", "rename", "old"];
        args.extend_from_slice(extra);
        let seen = run_in_pty_steps_with_env(
            &a,
            &args,
            vec![
                PtyStep::WaitFor("New slug"),
                PtyStep::Send("surrogate-model\r".to_owned()),
                PtyStep::WaitFor("Proceed?"),
                PtyStep::Do(Box::new(move || {
                    fs::write(b.join("notes.txt"), "from the other clone\n").unwrap();
                    git(&b, &["commit", "-am", "other clone"]);
                    git(&b, &["push"]);
                    git(&fetching, &["fetch"]);
                })),
                PtyStep::Send("y\r".to_owned()),
            ],
            &[("HOME", &home), ("XDG_CONFIG_HOME", &home)],
        );
        (tmp, a, seen)
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_fetch_during_the_confirm_prompt_is_behind_upstream() {
        require_git!();
        let (_tmp, a, seen) = fetch_during_the_confirm(&[]);
        assert!(seen.contains("1 commit behind its upstream"), "{seen}");
        assert!(a.join("projects/old.md").exists(), "nothing moved");
        assert!(!a.join("projects/surrogate-model.md").exists());

        let (_tmp, a, seen) = fetch_during_the_confirm(&["--skip-sync-check"]);
        assert!(renamed(&a), "the override lets it through: {seen}");
        assert!(
            seen.contains("renaming anyway (--skip-sync-check)"),
            "{seen}"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_paused_check_runs_again_under_the_lock() {
        // The check before the preview finds no merge; one is started while
        // the confirm waits, and the guard under the lock refuses it.
        require_git!();
        let tmp = tempdir().unwrap();
        let a = committed_vault(tmp.path());
        diverging_branches(&a);
        let in_merge = a.clone();
        let home = tmp.path().to_path_buf();
        let seen = run_in_pty_steps_with_env(
            &a,
            &["note", "rename", "old"],
            vec![
                PtyStep::WaitFor("New slug"),
                PtyStep::Send("surrogate-model\r".to_owned()),
                PtyStep::WaitFor("Proceed?"),
                PtyStep::Do(Box::new(move || start_conflicted_merge(&in_merge))),
                PtyStep::Send("y\r".to_owned()),
            ],
            &[("HOME", &home), ("XDG_CONFIG_HOME", &home)],
        );
        assert!(seen.contains("a git operation is in progress"), "{seen}");
        assert!(a.join("projects/old.md").exists(), "nothing moved");
        assert!(!a.join("projects/surrogate-model.md").exists());
    }
}

#[test]
fn note_create_format_typst() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());
    let body_file = dir.path().join("body.typ");
    fs::write(&body_file, "The error splits as $ sigma^2 $.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--format",
            "typst",
            "--title",
            "Bias-variance split",
            "--body-file",
            body_file.to_str().unwrap(),
            "--origin",
            "[[projects/surrogate-model]]",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("concepts/bias-variance-split.typ"));

    let content = fs::read_to_string(dir.path().join("concepts/bias-variance-split.typ")).unwrap();
    assert!(
        content.starts_with("#metadata((\n  type: \"concept\",\n"),
        "{content}"
    );
    assert!(
        content.contains("origin: \"[[projects/surrogate-model]]\""),
        "{content}"
    );
    assert!(
        content.ends_with("<cdno>\n\nThe error splits as $ sigma^2 $.\n"),
        "{content}"
    );
    assert!(!dir.path().join("concepts/bias-variance-split.md").exists());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--format",
            "typst",
            "--title",
            "Another",
            "--var",
            "x=y",
            "--body-file",
            body_file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Markdown notes only"));
}

#[test]
fn note_create_format_typst_without_a_body_file_fails_non_interactively() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());
    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--format",
            "typst",
            "--title",
            "Bias-variance split",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("body-file"));
    assert!(!dir.path().join("concepts/bias-variance-split.typ").exists());
}

/// A Typst concept, created through `cdno note create`; returns the vault argument.
fn create_typst_concept(dir: &Path) -> String {
    init_person_vault(dir);
    let v = vault_arg(dir);
    let body_file = dir.join("body.typ");
    fs::write(&body_file, "= Statement\n\nThe error splits.\n").unwrap();
    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--format",
            "typst",
            "--title",
            "Bias-variance split",
            "--body-file",
            body_file.to_str().unwrap(),
            "--origin",
            "[[projects/surrogate-model]]",
        ])
        .assert()
        .success();
    v
}

#[test]
fn note_revise_replaces_a_typst_body_and_keeps_the_block() {
    let dir = tempdir().unwrap();
    let v = create_typst_concept(dir.path());
    let note = dir.path().join("concepts/bias-variance-split.typ");
    let before = fs::read_to_string(&note).unwrap();
    let body_file = dir.path().join("new.typ");
    fs::write(&body_file, "= Statement\n\nA better account.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "revise",
            "concept:bias-variance-split",
            "--body-file",
            body_file.to_str().unwrap(),
            "--reason",
            "tightened the statement",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Revised concepts/bias-variance-split.typ",
        ));

    let block_end = before.find("<cdno>").unwrap() + "<cdno>".len();
    let after = fs::read_to_string(&note).unwrap();
    assert_eq!(after[..block_end], before[..block_end]);
    assert_eq!(
        after[block_end..],
        *"\n\n= Statement\n\nA better account.\n"
    );
}

#[test]
fn note_revise_section_on_typst_is_refused_with_the_remedy() {
    let dir = tempdir().unwrap();
    let v = create_typst_concept(dir.path());
    let note = dir.path().join("concepts/bias-variance-split.typ");
    let before = fs::read_to_string(&note).unwrap();
    let content = dir.path().join("section.typ");
    fs::write(&content, "New.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "revise",
            "concept:bias-variance-split",
            "--section",
            "Statement",
            "--content-file",
            content.to_str().unwrap(),
            "--reason",
            "tightened",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not by section"))
        .stderr(predicate::str::contains("--body-file"));

    assert_eq!(fs::read_to_string(&note).unwrap(), before);
}
