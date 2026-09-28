//! `cdno config note-type install` (RFC 0003): installing a bundled note type
//! into an existing vault.
//!
//! Subprocess runs against temp vaults, one per vault shape the RFC names:
//! fresh, pre-concept, hand-copied without a template, customised, deleted
//! declaration with notes kept, folder collision, no templates directory,
//! inline `note_types`. Each asserts the report, the files, and what
//! `--list` says about the vault afterwards.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use cdno_cli::bundled::{self, BundledType};
use cdno_cli::commands::init::{self, CONCEPT_TEMPLATE, CONCEPT_TYPE_BLOCK};
use tempfile::tempdir;

fn cdno(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd.env_remove("RUST_BACKTRACE");
    cmd.arg("--vault").arg(dir).arg("--no-interactive");
    cmd
}

fn config_path(root: &Path) -> PathBuf {
    root.join(".cuaderno/config.toml")
}

fn template_path(root: &Path) -> PathBuf {
    root.join(".cuaderno/templates/concept.md")
}

fn read_config(root: &Path) -> String {
    fs::read_to_string(config_path(root)).unwrap()
}

/// A vault as a binary from before the concept type (#644) left it: the
/// default config, `daily.md`, and no block, template or folder.
fn older_vault(root: &Path) {
    init::run(root).expect("init");
    let config = read_config(root);
    let stripped = config.replace(CONCEPT_TYPE_BLOCK, "");
    assert_ne!(stripped, config, "precondition: the block was present");
    fs::write(config_path(root), stripped).unwrap();
    fs::remove_file(template_path(root)).unwrap();
    fs::remove_dir(root.join("concepts")).unwrap();
}

/// Run `install --name concept` and return (success, stdout, stderr).
fn install(root: &Path, extra: &[&str]) -> (bool, String, String) {
    let out = cdno(root)
        .args(["config", "note-type", "install", "--name", "concept"])
        .args(extra)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

/// The `--list --json` state of `concept` in the vault.
fn listed_state(root: &Path) -> String {
    let out = cdno(root)
        .args(["--json", "config", "note-type", "install", "--list"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let row = value["bundled"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "concept")
        .expect("concept listed")
        .clone();
    row["state"].as_str().unwrap().to_owned()
}

/// Every entry under `root` with its bytes (`None` for a directory), sorted.
fn snapshot(root: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Option<Vec<u8>>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                out.push((rel, None));
                walk(root, &path, out);
            } else {
                out.push((rel, Some(fs::read(&path).unwrap())));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn validates(root: &Path) {
    cdno(root).args(["config", "validate"]).assert().success();
}

#[test]
fn a_fresh_install_writes_declaration_template_and_folder() {
    let dir = tempdir().unwrap();
    older_vault(dir.path());
    assert_eq!(listed_state(dir.path()), "not_installed");

    let (ok, stdout, stderr) = install(dir.path(), &[]);
    assert!(ok, "{stderr}");
    assert_eq!(
        stdout,
        "declaration  written\n\
         template     written .cuaderno/templates/concept.md\n\
         folder       created concepts/\n"
    );
    assert!(read_config(dir.path()).ends_with(CONCEPT_TYPE_BLOCK));
    assert_eq!(
        fs::read_to_string(template_path(dir.path())).unwrap(),
        CONCEPT_TEMPLATE
    );
    assert!(dir.path().join("concepts").is_dir());
    validates(dir.path());
    assert_eq!(listed_state(dir.path()), "installed_matches");

    // And a note of the type can now be created.
    let body = dir.path().join("body.md");
    fs::write(&body, "A body.\n").unwrap();
    cdno(dir.path())
        .args(["note", "create", "concept", "--title", "x", "--body-file"])
        .arg(&body)
        .assert()
        .success();
    assert!(dir.path().join("concepts/x.md").is_file());
}

#[test]
fn installing_into_an_initialised_vault_changes_nothing_and_exits_zero() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    let before = snapshot(dir.path());

    let (ok, stdout, stderr) = install(dir.path(), &[]);
    assert!(ok, "{stderr}");
    assert_eq!(
        stdout,
        "declaration  kept (matches bundled)\n\
         template     kept (matches bundled)\n\
         folder       present concepts/\n\
         concept: already installed, nothing to do\n"
    );
    assert_eq!(snapshot(dir.path()), before, "the vault is byte-identical");

    // A second install after a first behaves the same.
    let older = tempdir().unwrap();
    older_vault(older.path());
    assert!(install(older.path(), &[]).0);
    let once = snapshot(older.path());
    let (ok, stdout, _) = install(older.path(), &[]);
    assert!(ok);
    assert!(stdout.ends_with("concept: already installed, nothing to do\n"));
    assert_eq!(snapshot(older.path()), once);
}

#[test]
fn a_hand_copied_declaration_without_a_template_gets_the_template() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    fs::remove_file(template_path(dir.path())).unwrap();
    let config = read_config(dir.path());
    assert_eq!(listed_state(dir.path()), "installed_template_missing");

    let (ok, stdout, stderr) = install(dir.path(), &[]);
    assert!(ok, "{stderr}");
    assert_eq!(
        stdout,
        "declaration  kept (matches bundled)\n\
         template     written .cuaderno/templates/concept.md\n\
         folder       present concepts/\n"
    );
    assert_eq!(read_config(dir.path()), config, "config untouched");
    assert_eq!(
        fs::read_to_string(template_path(dir.path())).unwrap(),
        CONCEPT_TEMPLATE
    );
    assert_eq!(listed_state(dir.path()), "installed_matches");
}

#[test]
fn a_declaration_naming_another_template_file_gets_not_installed() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    fs::remove_file(template_path(dir.path())).unwrap();
    let config =
        read_config(dir.path()).replace(r#"template = "concept.md""#, r#"template = "idea.md""#);
    fs::write(config_path(dir.path()), &config).unwrap();
    assert_eq!(listed_state(dir.path()), "installed_declaration_differs");

    let (ok, stdout, stderr) = install(dir.path(), &[]);
    assert!(ok, "{stderr}");
    assert!(
        stdout.starts_with(
            "declaration  kept (differs: template: bundled \"concept.md\", yours \"idea.md\")\n"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains("cdno config note-type set --name concept --template concept.md"),
        "{stdout}"
    );
    assert!(
        stdout.contains("template     not installed (declaration names idea.md)\n"),
        "{stdout}"
    );
    assert!(!template_path(dir.path()).exists(), "nothing written");
    assert!(!dir.path().join(".cuaderno/templates/idea.md").exists());
    assert_eq!(read_config(dir.path()), config);
}

#[test]
fn a_customised_template_is_kept_and_reported() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    let custom = "---\ntype: concept\ncreated: {{created}}\n---\n\n# {{title}}\n\nMine.\n";
    fs::write(template_path(dir.path()), custom).unwrap();
    assert_eq!(listed_state(dir.path()), "installed_template_customised");

    let (ok, stdout, stderr) = install(dir.path(), &[]);
    assert!(ok, "{stderr}");
    assert!(
        stdout.contains("template     kept (customised)\n"),
        "{stdout}"
    );
    assert!(stdout.ends_with("concept: already installed, nothing to do\n"));
    assert_eq!(
        fs::read_to_string(template_path(dir.path())).unwrap(),
        custom
    );
}

#[test]
fn a_deleted_declaration_with_notes_kept_gets_the_declaration_only() {
    // RFC 0003 §6: the owner removed the block but kept notes and an edited
    // template. The declaration comes back; the template and notes stay.
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    let custom = format!("{CONCEPT_TEMPLATE}\n## Mine\n");
    fs::write(template_path(dir.path()), &custom).unwrap();
    fs::write(
        dir.path().join("concepts/kept.md"),
        "---\ntype: concept\ncreated: 2026-01-01\ntags: []\n---\n\n# Kept\n",
    )
    .unwrap();
    let config = read_config(dir.path()).replace(CONCEPT_TYPE_BLOCK, "");
    fs::write(config_path(dir.path()), config).unwrap();
    assert_eq!(listed_state(dir.path()), "not_installed");

    let (ok, stdout, stderr) = install(dir.path(), &[]);
    assert!(ok, "{stderr}");
    assert_eq!(
        stdout,
        "declaration  written\n\
         template     kept (customised)\n\
         folder       present concepts/\n"
    );
    assert_eq!(
        fs::read_to_string(template_path(dir.path())).unwrap(),
        custom
    );
    assert!(dir.path().join("concepts/kept.md").is_file());
    validates(dir.path());
    assert_eq!(listed_state(dir.path()), "installed_template_customised");
}

#[test]
fn a_folder_collision_refuses_and_leaves_nothing_behind() {
    let dir = tempdir().unwrap();
    older_vault(dir.path());
    let mut config = read_config(dir.path());
    config.push_str("[note_types.idea]\nfolder = \"concepts\"\n");
    fs::write(config_path(dir.path()), &config).unwrap();
    let before = snapshot(dir.path());

    let (ok, _stdout, stderr) = install(dir.path(), &[]);
    assert!(!ok, "a collision must fail");
    assert!(
        stderr.contains("another note type already uses the folder `concepts`"),
        "{stderr}"
    );
    assert!(stderr.contains("both declare folder"), "{stderr}");
    // The template written in step 1 and the folder from step 2 are gone.
    assert_eq!(snapshot(dir.path()), before, "nothing left behind");
    assert_eq!(listed_state(dir.path()), "not_installed");
}

#[test]
fn a_missing_templates_directory_is_created() {
    let dir = tempdir().unwrap();
    older_vault(dir.path());
    fs::remove_dir_all(dir.path().join(".cuaderno/templates")).unwrap();

    let (ok, stdout, stderr) = install(dir.path(), &[]);
    assert!(ok, "{stderr}");
    assert!(
        stdout.contains("template     written .cuaderno/templates/concept.md"),
        "{stdout}"
    );
    assert_eq!(
        fs::read_to_string(template_path(dir.path())).unwrap(),
        CONCEPT_TEMPLATE
    );
    assert_eq!(listed_state(dir.path()), "installed_matches");
}

#[test]
fn an_inline_note_types_table_gets_the_translated_message() {
    let dir = tempdir().unwrap();
    older_vault(dir.path());
    let config = format!(
        "note_types = {{ person = {{ folder = \"people\" }} }}\n{}",
        read_config(dir.path())
    );
    fs::write(config_path(dir.path()), &config).unwrap();
    validates(dir.path());
    let before = snapshot(dir.path());

    let (ok, _stdout, stderr) = install(dir.path(), &[]);
    assert!(!ok);
    assert!(
        stderr.contains("`note_types` is declared inline; add the block with `cdno config edit`"),
        "{stderr}"
    );
    assert_eq!(snapshot(dir.path()), before, "nothing left behind");
    assert_eq!(listed_state(dir.path()), "not_installed");
}

#[test]
fn a_commented_out_declaration_does_not_count_and_a_dotted_one_does() {
    // "Declared" is decided on the parsed config, never by a text search.
    let commented = tempdir().unwrap();
    older_vault(commented.path());
    let mut config = read_config(commented.path());
    config.push_str("# [note_types.concept]\n# folder = \"concepts\"\n");
    fs::write(config_path(commented.path()), &config).unwrap();
    assert_eq!(listed_state(commented.path()), "not_installed");
    let (ok, stdout, _) = install(commented.path(), &[]);
    assert!(ok);
    assert!(stdout.starts_with("declaration  written\n"), "{stdout}");
    validates(commented.path());

    let dotted = tempdir().unwrap();
    older_vault(dotted.path());
    let config = format!(
        "note_types.concept.folder = \"concepts\"\n{}",
        read_config(dotted.path())
    );
    fs::write(config_path(dotted.path()), &config).unwrap();
    validates(dotted.path());
    let (ok, stdout, _) = install(dotted.path(), &[]);
    assert!(ok);
    assert!(
        stdout.starts_with("declaration  kept (differs: "),
        "{stdout}"
    );
    assert_eq!(read_config(dotted.path()), config, "never modified");
}

#[test]
fn a_differing_declaration_is_kept_and_names_the_adopting_command() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    let config = read_config(dir.path())
        .replace(r#"optional = ["tags", "origin"]"#, r#"optional = ["tags"]"#);
    fs::write(config_path(dir.path()), &config).unwrap();

    let (ok, stdout, stderr) = install(dir.path(), &[]);
    assert!(ok, "{stderr}");
    assert!(
        stdout.starts_with(
            "declaration  kept (differs: optional: bundled [\"tags\", \"origin\"], yours [\"tags\"])\n\
             \x20            to adopt the bundled optional: \
             cdno config note-type set --name concept --optional tags,origin\n"
        ),
        "{stdout}"
    );
    assert_eq!(read_config(dir.path()), config, "never merged");
}

#[test]
fn json_follows_the_rfc_shape() {
    let dir = tempdir().unwrap();
    older_vault(dir.path());
    let out = cdno(dir.path())
        .args([
            "--json",
            "config",
            "note-type",
            "install",
            "--name",
            "concept",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "changed": true,
            "note_type": "concept",
            "declaration": "written",
            "template": {"path": "templates/concept.md", "action": "written"},
            "folder": {"path": "concepts", "action": "created"},
        })
    );

    let out = cdno(dir.path())
        .args([
            "--json",
            "config",
            "note-type",
            "install",
            "--name",
            "concept",
        ])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["changed"], false);
    assert_eq!(value["declaration"], "kept_matches");
    assert_eq!(value["template"]["action"], "kept_matches");
    assert_eq!(value["folder"]["action"], "present");
}

#[test]
fn list_prints_the_type_without_template_bodies() {
    let dir = tempdir().unwrap();
    older_vault(dir.path());
    let out = cdno(dir.path())
        .args(["config", "note-type", "install", "--list"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("concept: not installed\n"), "{stdout}");
    assert!(stdout.contains("  folder    concepts/\n"), "{stdout}");
    assert!(stdout.contains("  required  created\n"), "{stdout}");
    assert!(stdout.contains("  optional  tags, origin\n"), "{stdout}");
    assert!(
        stdout.contains("  template  concept.md (sections: Statement, Why it matters, See also)"),
        "{stdout}"
    );
    assert!(!stdout.contains("{{"), "no template body: {stdout}");

    // `--list` and `--name` are exclusive.
    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "install",
            "--list",
            "--name",
            "concept",
        ])
        .assert()
        .failure();
}

#[test]
fn dry_run_prints_what_would_be_written_and_writes_nothing() {
    let dir = tempdir().unwrap();
    older_vault(dir.path());
    let before = snapshot(dir.path());

    let (ok, stdout, stderr) = install(dir.path(), &["--dry-run"]);
    assert!(ok, "{stderr}");
    assert!(stdout.contains("nothing was written"), "{stdout}");
    assert!(stdout.contains(CONCEPT_TYPE_BLOCK), "{stdout}");
    assert!(stdout.contains(CONCEPT_TEMPLATE), "{stdout}");
    assert_eq!(snapshot(dir.path()), before, "a dry run writes nothing");

    // A dry run into a vault that would refuse reports the refusal.
    let mut config = read_config(dir.path());
    config.push_str("[note_types.idea]\nfolder = \"concepts\"\n");
    fs::write(config_path(dir.path()), &config).unwrap();
    let before = snapshot(dir.path());
    let (ok, _, stderr) = install(dir.path(), &["--dry-run"]);
    assert!(!ok);
    assert!(stderr.contains("already uses the folder"), "{stderr}");
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn a_missing_name_off_a_terminal_names_the_flag() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    let out = cdno(dir.path())
        .args(["config", "note-type", "install"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("missing required flag: --name"), "{stderr}");

    let out = cdno(dir.path())
        .args(["config", "note-type", "install", "--name", "person"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("'person' is not a bundled note type — bundled: concept"),
        "{stderr}"
    );
}

#[test]
fn the_gate_holds_for_an_invalid_block() {
    // The `const` registry cannot be given a bad entry from a subprocess, so
    // drive the install directly with one: a block claiming a reserved
    // folder, which the config gate must refuse. The template written in
    // step 1 and the folder from step 2 must go again, and the config must
    // be untouched.
    let dir = tempdir().unwrap();
    older_vault(dir.path());
    let before = snapshot(dir.path());
    let bad = BundledType {
        name: "gadget",
        purpose: "a test entry",
        block: "[note_types.gadget]\nfolder = \"gadgets\"\nrequired = [\"created\"]\n\
                title_field = 5\n",
        template_filename: "gadget.md",
        template: "---\ntype: gadget\n---\n",
    };
    // A block that does not even parse as the model is refused before any
    // write.
    assert!(bundled::install(dir.path(), &bad, false).is_err());
    assert_eq!(snapshot(dir.path()), before);

    let reserved = BundledType {
        block: "[note_types.gadget]\nfolder = \"journal/gadgets\"\n",
        ..bad
    };
    let err = bundled::install(dir.path(), &reserved, false).expect_err("gate refuses");
    assert!(err.to_string().contains("nothing was written"), "{err:#}");
    assert_eq!(snapshot(dir.path()), before, "nothing written");
    assert!(!dir.path().join(".cuaderno/templates/gadget.md").exists());
}
