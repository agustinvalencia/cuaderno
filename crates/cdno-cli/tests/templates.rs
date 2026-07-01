//! In-process tests for `cdno templates vars`. Seed a vault on disk, then
//! assert on the `placeholders` data seam / `render_table` / `json_rows`
//! rather than capturing stdout (same pattern as `cdno search`).

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

    let ph = templates::placeholders(dir.path(), "project", None).expect("placeholders");
    let names: Vec<&str> = ph.iter().map(|p| p.name.as_str()).collect();
    // The acceptance set for `project` (#271).
    assert_eq!(
        names,
        ["context", "status", "created", "core_question", "title"]
    );

    let table = templates::render_table(&ph);
    assert!(table.contains("{{title}}"), "table:\n{table}");
    assert!(table.contains("supplied"), "table:\n{table}");
}

#[test]
fn templates_vars_json_rows_are_a_stable_array() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let ph = templates::placeholders(dir.path(), "project", None).expect("placeholders");
    let rows = templates::json_rows(&ph);
    assert_eq!(rows.len(), 5, "project supplies five placeholders");
    assert_eq!(rows[0]["name"], "context");
    assert_eq!(rows[0]["source"], "supplied");
    // `message` is only present on prompt rows.
    assert!(rows[0].get("message").is_none());
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

    let ph = templates::placeholders(dir.path(), "project", None).expect("placeholders");
    let rows = templates::json_rows(&ph);
    let author = rows
        .iter()
        .find(|r| r["name"] == "author")
        .expect("author listed");
    assert_eq!(author["source"], "config");
    let ticket = rows
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

    let ph = templates::placeholders(dir.path(), "tracking", Some("gym")).expect("placeholders");
    let names: Vec<&str> = ph.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"routine"),
        "gym variant supplies routine: {names:?}"
    );
}

#[test]
fn templates_vars_rejects_an_unknown_type() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = templates::placeholders(dir.path(), "bogus", None).expect_err("should error");
    let msg = err.to_string();
    assert!(msg.contains("unknown note type"), "msg: {msg}");
    assert!(msg.contains("project"), "should list valid types: {msg}");
}

#[test]
fn templates_eject_materialises_the_builtin() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let path = templates::eject(dir.path(), "project", None, false).expect("eject");
    assert_eq!(path, ".cuaderno/templates/project.md");
    let content = fs::read_to_string(dir.path().join(&path)).unwrap();
    assert!(content.contains("## Current State"), "content:\n{content}");
    assert!(content.contains("{{title}}"), "content:\n{content}");
}

#[test]
fn templates_eject_refuses_to_clobber_then_force_overwrites() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let target = dir.path().join(".cuaderno/templates/project.md");
    fs::write(&target, "# mine\n").unwrap();

    let err = templates::eject(dir.path(), "project", None, false).expect_err("should refuse");
    assert!(err.to_string().contains("already exists"), "msg: {err}");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# mine\n",
        "left untouched"
    );

    templates::eject(dir.path(), "project", None, true).expect("force eject");
    assert!(
        fs::read_to_string(&target)
            .unwrap()
            .contains("## Current State")
    );
}

#[test]
fn templates_eject_variant_and_unknown_variant_error() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let path = templates::eject(dir.path(), "tracking", Some("gym"), false).expect("gym");
    assert_eq!(path, ".cuaderno/templates/tracking-gym.md");
    assert!(dir.path().join(&path).exists());

    let err =
        templates::eject(dir.path(), "tracking", Some("deadlift"), false).expect_err("no builtin");
    assert!(err.to_string().contains("variant 'deadlift'"), "msg: {err}");
}
