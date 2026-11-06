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

    fn git_available() -> bool {
        Proc::new("git").arg("--version").output().is_ok()
    }

    /// Run `git` in `dir` as a throwaway identity, never the global one.
    fn git(dir: &Path, args: &[&str]) {
        let out = Proc::new("git")
            .current_dir(dir)
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
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} in {}: {}{}",
            dir.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
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

        // Clone A takes the archived note and opens on this build once (the
        // backfill runs), then renames. The CLI verb is T24, so the domain
        // verb is driven directly through the composition root.
        git(&a, &["pull", "--no-rebase"]);
        cdno_at(&a).arg("lint").assert().success();
        let now = chrono::Local::now().naive_local();
        {
            let opened = cdno_domain::bootstrap::open_vault(&a).expect("open A");
            opened
                .vault
                .rename_note("old", "surrogate-model", None, None, now)
                .expect("rename on A");
        }
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
