use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::{Context, ProjectStatus};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn project_body(context: &str, status: &str, created: &str, title: &str) -> String {
    format!(
        "---\ntype: project\ncontext: {context}\nstatus: {status}\ncreated: {created}\n---\n# {title}\n"
    )
}

fn vault_with_notes(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

#[test]
fn active_projects_returns_empty_for_empty_vault() {
    let vault = vault_with_notes(&[]);

    let active = vault.active_projects().expect("query succeeds");

    assert!(active.is_empty());
}

#[test]
fn active_projects_returns_only_active_status() {
    let active_a = project_body("work", "active", "2026-01-10", "A");
    let active_b = project_body("personal", "active", "2026-02-01", "B");
    let parked = project_body("work", "parked", "2026-01-15", "P");
    let completed = project_body("legal", "completed", "2026-01-20", "C");

    let vault = vault_with_notes(&[
        ("projects/a.md", &active_a),
        ("projects/b.md", &active_b),
        ("projects/_parked/p.md", &parked),
        ("projects/c.md", &completed),
    ]);

    let active = vault.active_projects().expect("query succeeds");

    assert_eq!(active.len(), 2, "got: {active:?}");
    let mut paths: Vec<_> = active.iter().map(|(p, _)| p.to_string()).collect();
    paths.sort();
    assert_eq!(paths, vec!["projects/a.md", "projects/b.md"]);
}

#[test]
fn active_projects_ignores_other_note_types() {
    // A daily note in the vault must not be picked up by the project
    // query — even if (hypothetically) it had a `status: active` line.
    let project = project_body("work", "active", "2026-01-10", "A");
    let daily = "---\ntype: daily\ndate: 2026-04-28\n---\n# Today\n";
    let inbox = "---\ntype: inbox\ncreated: 2026-04-28\n---\n# Capture\n";

    let vault = vault_with_notes(&[
        ("projects/a.md", &project),
        ("journal/daily/2026-04-28.md", daily),
        ("inbox/note.md", inbox),
    ]);

    let active = vault.active_projects().expect("query succeeds");

    assert_eq!(active.len(), 1);
    assert_eq!(active[0].0, vp("projects/a.md"));
}

#[test]
fn active_projects_returns_typed_frontmatter() {
    let project = project_body("side-project", "active", "2026-03-05", "Side");
    let vault = vault_with_notes(&[("projects/side.md", &project)]);

    let active = vault.active_projects().expect("query succeeds");

    assert_eq!(active.len(), 1);
    let (_, fm) = &active[0];
    assert_eq!(fm.context, Context::SideProject);
    assert_eq!(fm.status, ProjectStatus::Active);
    assert_eq!(
        fm.created,
        chrono::NaiveDate::from_ymd_opt(2026, 3, 5).unwrap()
    );
    assert!(fm.core_question.is_none());
}

#[test]
fn active_projects_propagates_malformed_frontmatter_error() {
    // A project file missing `status` should fail the query rather
    // than silently disappear — silently skipping would let the user
    // bypass the 5-cap by writing a sixth project under a broken file.
    let bad = "---\ntype: project\ncontext: work\ncreated: 2026-01-10\n---\n# Bad\n";
    let vault = vault_with_notes(&[("projects/bad.md", bad)]);

    let result = vault.active_projects();

    assert!(
        result.is_err(),
        "expected validation error, got: {result:?}"
    );
}
