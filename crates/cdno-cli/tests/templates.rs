//! In-process tests for `cdno templates vars`. Seed a vault on disk, then
//! assert on the text from the `build_vars` seam rather than capturing
//! stdout (same pattern as `cdno search` / `cdno orient`).

use std::fs;
use std::path::Path;

use cdno_cli::commands::{init, templates};
use tempfile::tempdir;

fn seed(root: &Path) {
    init::run(root).expect("init");
}

#[test]
fn templates_vars_lists_the_project_supplied_placeholders() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let out = templates::build_vars(dir.path(), "project", None, false).expect("build");
    // The acceptance set for `project` (#271).
    for name in ["title", "context", "status", "created", "core_question"] {
        assert!(
            out.contains(&format!("{{{{{name}}}}}")),
            "missing {name}:\n{out}"
        );
    }
    assert!(out.contains("supplied"), "output:\n{out}");
}

#[test]
fn templates_vars_json_is_a_stable_array() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let out = templates::build_vars(dir.path(), "project", None, true).expect("build");
    let rows: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let arr = rows.as_array().expect("array");
    assert_eq!(arr.len(), 5, "project supplies five placeholders");
    assert_eq!(arr[0]["name"], "context");
    assert_eq!(arr[0]["source"], "supplied");
}

#[test]
fn templates_vars_surfaces_config_and_prompt_vars() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    // Append config variables to the seeded vault's config.
    let cfg = dir.path().join(".cuaderno/config.toml");
    let mut body = fs::read_to_string(&cfg).unwrap();
    body.push_str("\n[variables]\nauthor = \"A. Researcher\"\n\n[variables.prompt]\nticket = \"Ticket ID?\"\n");
    fs::write(&cfg, body).unwrap();

    let out = templates::build_vars(dir.path(), "project", None, true).expect("build");
    let rows: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = rows.as_array().unwrap();
    let author = arr
        .iter()
        .find(|r| r["name"] == "author")
        .expect("author listed");
    assert_eq!(author["source"], "config");
    let ticket = arr
        .iter()
        .find(|r| r["name"] == "ticket")
        .expect("ticket listed");
    assert_eq!(ticket["source"], "prompt");
    assert_eq!(ticket["message"], "Ticket ID?");
}

#[test]
fn templates_vars_resolves_a_tracking_variant() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let out = templates::build_vars(dir.path(), "tracking", Some("gym"), false).expect("build");
    assert!(
        out.contains("{{routine}}"),
        "gym variant supplies routine:\n{out}"
    );
}

#[test]
fn templates_vars_rejects_an_unknown_type() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = templates::build_vars(dir.path(), "bogus", None, false).expect_err("should error");
    let msg = err.to_string();
    assert!(msg.contains("unknown note type"), "msg: {msg}");
    assert!(msg.contains("project"), "should list valid types: {msg}");
}
