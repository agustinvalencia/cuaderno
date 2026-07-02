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

    let ph = templates::placeholders(dir.path(), "project").expect("placeholders");
    let names: Vec<&str> = ph.iter().map(|p| p.name.as_str()).collect();
    // The acceptance set for `project` (#271).
    assert_eq!(
        names,
        ["title", "context", "status", "created", "core_question"]
    );

    let table = templates::render_table(&ph);
    assert!(table.contains("{{title}}"), "table:\n{table}");
    assert!(table.contains("supplied"), "table:\n{table}");
}

#[test]
fn templates_vars_json_rows_are_a_stable_array() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let ph = templates::placeholders(dir.path(), "project").expect("placeholders");
    let rows = templates::json_rows(&ph);
    assert_eq!(rows.len(), 5, "project supplies five placeholders");
    assert_eq!(rows[0]["name"], "title");
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

    let ph = templates::placeholders(dir.path(), "project").expect("placeholders");
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
fn templates_vars_tracking_lists_the_complete_supplied_set() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    // The supplied set is the type's full create-path key set (#279), so it
    // includes `routine` and `activity_title` even though the generic built-in
    // template doesn't reference them.
    let ph = templates::placeholders(dir.path(), "tracking").expect("placeholders");
    let names: Vec<&str> = ph.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"activity_title"), "supplied set: {names:?}");
    assert!(names.contains(&"routine"), "supplied set: {names:?}");
}

#[test]
fn templates_vars_rejects_an_unknown_type() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = templates::placeholders(dir.path(), "bogus").expect_err("should error");
    let msg = err.to_string();
    assert!(msg.contains("unknown note type"), "msg: {msg}");
    assert!(msg.contains("project"), "should list valid types: {msg}");
}

#[test]
fn templates_eject_materialises_the_builtin() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let path = templates::eject(dir.path(), "project", false).expect("eject");
    assert_eq!(path, ".cuaderno/templates/project.md");
    let content = fs::read_to_string(dir.path().join(&path)).unwrap();
    // Byte-identical to the built-in — the guarantee the docs make (a note
    // created straight after ejecting is unchanged). Compares the on-disk FS
    // write against the compiled-in source template.
    assert_eq!(
        content,
        include_str!("../../cdno-domain/templates/project.md"),
        "ejected file must be byte-identical to the built-in"
    );
}

#[test]
fn templates_eject_refuses_to_clobber_then_force_overwrites() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let target = dir.path().join(".cuaderno/templates/project.md");
    fs::write(&target, "# mine\n").unwrap();

    let err = templates::eject(dir.path(), "project", false).expect_err("should refuse");
    assert!(err.to_string().contains("already exists"), "msg: {err}");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# mine\n",
        "left untouched"
    );

    templates::eject(dir.path(), "project", true).expect("force eject");
    assert!(
        fs::read_to_string(&target)
            .unwrap()
            .contains("## Current State")
    );
}

#[test]
fn templates_eject_tracking_writes_the_generic_template() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    // Only base note-type templates eject (no `--variant` flag): the generic
    // tracking template is written. Activity variants are authored in the
    // vault, not ejected.
    let path = templates::eject(dir.path(), "tracking", false).expect("base tracking");
    assert_eq!(path, ".cuaderno/templates/tracking.md");
    assert!(dir.path().join(&path).exists());
}

#[test]
fn templates_eject_all_writes_every_type_skipping_existing() {
    let dir = tempdir().unwrap();
    seed(dir.path()); // init seeds daily.md

    let report = templates::eject_all(dir.path(), false).expect("eject all");
    // All 11 types end up on disk; daily was already seeded, so it's skipped.
    assert_eq!(report.written.len() + report.skipped.len(), 11);
    assert_eq!(report.skipped, vec!["daily"]);
    assert!(report.written.contains(&"project".to_owned()));
    let templates_dir = dir.path().join(".cuaderno/templates");
    for t in [
        "project",
        "action",
        "tracking",
        "commitment",
        "inbox",
        "daily",
    ] {
        assert!(
            templates_dir.join(format!("{t}.md")).exists(),
            "missing {t}.md"
        );
    }
}

#[test]
fn templates_eject_all_rerun_skips_everything() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    templates::eject_all(dir.path(), false).expect("first");
    let second = templates::eject_all(dir.path(), false).expect("second");
    assert!(second.written.is_empty(), "everything already exists");
    assert_eq!(second.skipped.len(), 11);
}

#[test]
fn templates_eject_all_force_overwrites_a_customised_template() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let project = dir.path().join(".cuaderno/templates/project.md");
    fs::write(&project, "# mine\n").unwrap();

    let report = templates::eject_all(dir.path(), true).expect("force eject all");
    assert_eq!(report.written.len(), 11, "force writes all, none skipped");
    assert!(report.skipped.is_empty());
    // The customised project.md was overwritten with the built-in.
    assert!(
        fs::read_to_string(&project)
            .unwrap()
            .contains("## Current State"),
        "project.md overwritten"
    );
}
