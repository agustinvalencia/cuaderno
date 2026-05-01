use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{Context, ProjectFrontmatter, ProjectStatus};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn project_body(context: &str, status: &str, created: &str, title: &str) -> String {
    format!(
        "---\ntype: project\ncontext: {context}\nstatus: {status}\ncreated: {created}\n---\n# {title}\n"
    )
}

fn vault_with_notes(notes: &[(&str, &str)]) -> Vault {
    vault_with_notes_and_config(notes, VaultConfig::default())
}

fn vault_with_notes_and_config(notes: &[(&str, &str)], config: VaultConfig) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, config).expect("Vault::new");
    vault
}

/// Build a vault and keep an `Arc` to the seeded store, so tests can
/// read back what `create_project` wrote. The store inside `Vault`
/// is private, so reaching through the constructor is the cleanest
/// way to assert on file contents.
fn vault_with_seeded_store(
    notes: &[(&str, &str)],
    config: VaultConfig,
) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(Arc::clone(&store), index, config).expect("Vault::new");
    (vault, store)
}

fn read_project_frontmatter(store: &Arc<dyn VaultStore>, path: &VaultPath) -> ProjectFrontmatter {
    let raw = store.read_file(path).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    ProjectFrontmatter::try_from(fm).unwrap()
}

fn config_with_cap(max: u8) -> VaultConfig {
    let mut cfg = VaultConfig::default();
    cfg.vault.max_active_projects = max;
    cfg
}

fn day(year: i32, month: u32, day: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap()
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

// ---------------------------------------------------------------------
// create_project
// ---------------------------------------------------------------------

#[test]
fn create_project_writes_an_active_project_at_projects_slash_slug() {
    let (vault, store) = vault_with_seeded_store(&[], VaultConfig::default());

    let path = vault
        .create_project(day(2026, 4, 28), "ICML paper", Context::Work, None)
        .expect("create succeeds");

    assert_eq!(path, vp("projects/icml-paper.md"));
    let fm = read_project_frontmatter(&store, &path);
    assert_eq!(fm.context, Context::Work);
    assert_eq!(fm.status, ProjectStatus::Active);
    assert_eq!(fm.created, day(2026, 4, 28));
    assert!(fm.core_question.is_none());
}

#[test]
fn create_project_with_core_question_wraps_target_in_wikilink() {
    let (vault, store) = vault_with_seeded_store(&[], VaultConfig::default());

    let path = vault
        .create_project(
            day(2026, 4, 28),
            "Surrogate Model",
            Context::Work,
            Some("questions/research/surrogate-cost"),
        )
        .expect("create succeeds");

    let fm = read_project_frontmatter(&store, &path);
    assert_eq!(
        fm.core_question.as_deref(),
        Some("[[questions/research/surrogate-cost]]"),
    );
}

#[test]
fn create_project_indexes_the_new_note_so_active_projects_picks_it_up() {
    let (vault, _store) = vault_with_seeded_store(&[], VaultConfig::default());

    vault
        .create_project(day(2026, 4, 28), "First", Context::Personal, None)
        .expect("create succeeds");

    let active = vault.active_projects().expect("query succeeds");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].0, vp("projects/first.md"));
}

#[test]
fn create_project_seeds_parked_when_active_count_at_cap() {
    // Cap of 2 with 2 projects already active — the third still
    // succeeds, but is seeded as parked so the cap (on actives) is
    // preserved without blocking capture.
    let cfg = config_with_cap(2);
    let a = project_body("work", "active", "2026-01-10", "Alpha");
    let b = project_body("personal", "active", "2026-02-01", "Beta");
    let (vault, store) =
        vault_with_seeded_store(&[("projects/alpha.md", &a), ("projects/beta.md", &b)], cfg);

    let path = vault
        .create_project(day(2026, 4, 28), "Gamma", Context::Work, None)
        .expect("create succeeds, seeded as parked");

    assert_eq!(path, vp("projects/_parked/gamma.md"));
    let fm = read_project_frontmatter(&store, &path);
    assert_eq!(fm.status, ProjectStatus::Parked);

    let active = vault.active_projects().expect("query succeeds");
    assert_eq!(
        active.len(),
        2,
        "active count must be unchanged: parked seed must not consume a slot"
    );
}

#[test]
fn create_project_errors_when_slug_collides_with_parked_project() {
    // Slug uniqueness spans both projects/ and projects/_parked/ —
    // creating an active project whose slug already exists as parked
    // would make the activate/park flow ambiguous.
    let parked = project_body("work", "parked", "2026-01-10", "Same Title");
    let (vault, _store) = vault_with_seeded_store(
        &[("projects/_parked/same-title.md", &parked)],
        VaultConfig::default(),
    );

    let err = vault
        .create_project(day(2026, 4, 28), "Same Title", Context::Work, None)
        .unwrap_err();

    assert!(
        matches!(err, DomainError::Store(_)),
        "expected Store(AlreadyExists), got {err:?}"
    );
}

#[test]
fn create_project_does_not_count_parked_or_completed_against_cap() {
    let cfg = config_with_cap(1);
    let parked = project_body("work", "parked", "2026-01-10", "Parked");
    let completed = project_body("work", "completed", "2026-02-01", "Completed");
    let (vault, _store) = vault_with_seeded_store(
        &[
            ("projects/_parked/parked.md", &parked),
            ("projects/completed.md", &completed),
        ],
        cfg,
    );

    // Cap is 1 and there are 0 active — should succeed.
    vault
        .create_project(day(2026, 4, 28), "New", Context::Personal, None)
        .expect("create succeeds despite parked/completed already on disk");
}

#[test]
fn create_project_errors_when_filename_already_exists() {
    let (vault, _store) = vault_with_seeded_store(&[], VaultConfig::default());

    vault
        .create_project(day(2026, 4, 28), "Same Title", Context::Work, None)
        .expect("first create succeeds");

    let err = vault
        .create_project(day(2026, 4, 29), "Same Title", Context::Personal, None)
        .unwrap_err();

    assert!(
        matches!(err, DomainError::Store(_)),
        "expected Store(AlreadyExists), got {err:?}"
    );
}

#[test]
fn create_project_substitutes_kebab_case_for_multi_word_context() {
    let (vault, store) = vault_with_seeded_store(&[], VaultConfig::default());

    let path = vault
        .create_project(day(2026, 4, 28), "Side hustle", Context::SideProject, None)
        .expect("create succeeds");

    let raw = store.read_file(&path).unwrap();
    assert!(
        raw.contains("context: side-project"),
        "expected kebab-case 'side-project' in YAML, got:\n{raw}"
    );
}
