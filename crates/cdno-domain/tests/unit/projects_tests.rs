use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{Context, EnergyLevel, ProjectFrontmatter, ProjectStatus};

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

// ---------------------------------------------------------------------
// update_project_state
// ---------------------------------------------------------------------

fn project_body_with_state(
    context: &str,
    status: &str,
    created: &str,
    title: &str,
    state: &str,
) -> String {
    format!(
        "---\ntype: project\ncontext: {context}\nstatus: {status}\ncreated: {created}\n---\n\n# {title}\n\n## Current State\n{state}\n\n## Next Actions\n- [ ] First step\n",
    )
}

fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_hms_opt(hour, minute, 0)
        .unwrap()
}

#[test]
fn update_project_state_replaces_current_state_section() {
    let body = project_body_with_state(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "Started feature set B exploration.",
    );
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .update_project_state(
            dt(2026, 5, 1, 14, 32),
            "icml-paper",
            "Ablation confirmed; full geometry next.",
        )
        .expect("update succeeds");

    let raw = store.read_file(&vp("projects/icml-paper.md")).unwrap();
    assert!(
        raw.contains("Ablation confirmed; full geometry next."),
        "new state must be in project body:\n{raw}"
    );
    assert!(
        !raw.contains("Started feature set B exploration."),
        "old state must be gone:\n{raw}"
    );
}

#[test]
fn update_project_state_logs_was_now_to_daily_note() {
    let body = project_body_with_state(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "Started feature set B exploration.",
    );
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .update_project_state(
            dt(2026, 5, 1, 14, 32),
            "icml-paper",
            "Ablation confirmed; full geometry next.",
        )
        .expect("update succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-01.md"))
        .expect("daily note exists");
    assert!(
        daily.contains("- **14:32**: state on [[icml-paper]]"),
        "missing bullet head:\n{daily}"
    );
    assert!(
        daily.contains("was: Started feature set B exploration."),
        "missing was line:\n{daily}"
    );
    assert!(
        daily.contains("now: Ablation confirmed; full geometry next."),
        "missing now line:\n{daily}"
    );
}

#[test]
fn update_project_state_creates_daily_note_when_absent() {
    let body = project_body_with_state("work", "active", "2026-04-01", "ICML paper", "Initial.");
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .update_project_state(dt(2026, 5, 1, 9, 0), "icml-paper", "Updated.")
        .expect("update succeeds");

    assert!(
        store
            .exists(&vp("journal/2026/daily/2026-05-01.md"))
            .unwrap()
    );
}

#[test]
fn update_project_state_appends_when_daily_note_exists() {
    let body = project_body_with_state("work", "active", "2026-04-01", "ICML paper", "Initial.");
    let daily_existing = "---\ndate: 2026-05-01\ntype: daily\n---\n\n# Friday, 1 May 2026\n\n## Logs\n- **08:00**: standup\n";
    let (vault, store) = vault_with_seeded_store(
        &[
            ("projects/icml-paper.md", &body),
            ("journal/2026/daily/2026-05-01.md", daily_existing),
        ],
        VaultConfig::default(),
    );

    vault
        .update_project_state(dt(2026, 5, 1, 9, 0), "icml-paper", "Updated.")
        .expect("update succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-01.md"))
        .unwrap();
    assert!(
        daily.contains("- **08:00**: standup"),
        "earlier log must be preserved:\n{daily}"
    );
    assert!(
        daily.contains("- **09:00**: state on [[icml-paper]]"),
        "new log must be appended:\n{daily}"
    );
}

#[test]
fn update_project_state_errors_when_project_parked() {
    let body = project_body_with_state(
        "work",
        "parked",
        "2026-04-01",
        "Old idea",
        "Was active last quarter.",
    );
    let (vault, _store) = vault_with_seeded_store(
        &[("projects/_parked/old-idea.md", &body)],
        VaultConfig::default(),
    );

    let err = vault
        .update_project_state(dt(2026, 5, 1, 9, 0), "old-idea", "anything")
        .unwrap_err();

    assert!(
        matches!(&err, DomainError::ProjectNotActive(s) if s == "old-idea"),
        "got {err:?}"
    );
}

#[test]
fn update_project_state_errors_when_project_not_found() {
    let (vault, _store) = vault_with_seeded_store(&[], VaultConfig::default());

    let err = vault
        .update_project_state(dt(2026, 5, 1, 9, 0), "ghost", "anything")
        .unwrap_err();

    assert!(
        matches!(
            err,
            DomainError::Store(cdno_core::error::StoreError::NotFound(_))
        ),
        "got {err:?}"
    );
}

#[test]
fn update_project_state_errors_when_status_mismatches_folder() {
    // File lives at projects/x.md (active folder) but frontmatter
    // says parked. Frontmatter is the source of truth — refuse.
    let body = project_body_with_state("work", "parked", "2026-04-01", "Mismatched", "Initial.");
    let (vault, _store) =
        vault_with_seeded_store(&[("projects/mismatched.md", &body)], VaultConfig::default());

    let err = vault
        .update_project_state(dt(2026, 5, 1, 9, 0), "mismatched", "anything")
        .unwrap_err();

    assert!(
        matches!(err, DomainError::ProjectNotActive(_)),
        "got {err:?}"
    );
}

#[test]
fn update_project_state_errors_when_current_state_section_missing() {
    let body = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# No Section\n\n## Next Actions\n- [ ] step\n";
    let (vault, _store) =
        vault_with_seeded_store(&[("projects/no-section.md", body)], VaultConfig::default());

    let err = vault
        .update_project_state(dt(2026, 5, 1, 9, 0), "no-section", "new")
        .unwrap_err();

    assert!(matches!(err, DomainError::Manipulation(_)), "got {err:?}");
}

#[test]
fn update_project_state_preserves_other_sections() {
    let body = project_body_with_state("work", "active", "2026-04-01", "ICML paper", "Initial.");
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .update_project_state(dt(2026, 5, 1, 9, 0), "icml-paper", "Updated.")
        .expect("update succeeds");

    let raw = store.read_file(&vp("projects/icml-paper.md")).unwrap();
    assert!(raw.contains("## Next Actions"));
    assert!(raw.contains("- [ ] First step"));
}

#[test]
fn update_project_state_flattens_multiline_state_in_log_entry() {
    let multiline = "Feature set B ablation confirmed.\nSparse variant matches dense within 3%.\nNext step: full geometry.";
    let body = project_body_with_state("work", "active", "2026-04-01", "ICML paper", multiline);
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .update_project_state(dt(2026, 5, 1, 9, 0), "icml-paper", "Single line update.")
        .expect("update succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-01.md"))
        .unwrap();
    assert!(
        daily.contains("was: Feature set B ablation confirmed. Sparse variant matches dense within 3%. Next step: full geometry."),
        "multiline state must collapse to a single was: line:\n{daily}"
    );
}

#[test]
fn update_project_state_is_noop_when_state_unchanged() {
    let body = project_body_with_state("work", "active", "2026-04-01", "ICML paper", "Same state.");
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .update_project_state(dt(2026, 5, 1, 9, 0), "icml-paper", "Same state.\n")
        .expect("noop returns Ok");

    assert!(
        !store
            .exists(&vp("journal/2026/daily/2026-05-01.md"))
            .unwrap(),
        "noop must not create a daily note"
    );
    let raw = store.read_file(&vp("projects/icml-paper.md")).unwrap();
    assert_eq!(raw, body, "noop must not rewrite the project file");
}

// ---------------------------------------------------------------------
// add_action
// ---------------------------------------------------------------------

fn project_body_with_actions(
    context: &str,
    status: &str,
    created: &str,
    title: &str,
    actions_section: &str,
) -> String {
    format!(
        "---\ntype: project\ncontext: {context}\nstatus: {status}\ncreated: {created}\n---\n\n# {title}\n\n## Current State\nInitial.\n\n## Next Actions\n{actions_section}\n## Waiting On\n(nothing)\n",
    )
}

#[test]
fn add_action_appends_bullet_with_energy_tag() {
    let body = project_body_with_actions(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "- [ ] Existing step (light)\n",
    );
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .add_action(
            dt(2026, 5, 1, 14, 0),
            "icml-paper",
            "Run feature set B",
            EnergyLevel::Deep,
        )
        .expect("add_action succeeds");

    let raw = store.read_file(&vp("projects/icml-paper.md")).unwrap();
    assert!(
        raw.contains("- [ ] Existing step (light)"),
        "old action preserved:\n{raw}"
    );
    assert!(
        raw.contains("- [ ] Run feature set B (deep)"),
        "new action present:\n{raw}"
    );
}

#[test]
fn add_action_logs_addition_to_daily_note() {
    let body = project_body_with_actions("work", "active", "2026-04-01", "ICML paper", "");
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .add_action(
            dt(2026, 5, 1, 14, 0),
            "icml-paper",
            "Email supervisor",
            EnergyLevel::Light,
        )
        .expect("add_action succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-01.md"))
        .expect("daily note exists");
    assert!(
        daily.contains("- **14:00**: action added to [[icml-paper]] — Email supervisor (light)"),
        "missing log line:\n{daily}"
    );
}

#[test]
fn add_action_preserves_other_sections() {
    let body = project_body_with_actions(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "- [ ] Existing (light)\n",
    );
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .add_action(
            dt(2026, 5, 1, 14, 0),
            "icml-paper",
            "New",
            EnergyLevel::Medium,
        )
        .expect("add_action succeeds");

    let raw = store.read_file(&vp("projects/icml-paper.md")).unwrap();
    assert!(raw.contains("## Current State"));
    assert!(raw.contains("Initial."));
    assert!(raw.contains("## Waiting On"));
}

#[test]
fn add_action_errors_when_project_parked() {
    let body = project_body_with_actions(
        "work",
        "parked",
        "2026-04-01",
        "Old",
        "- [ ] Step (light)\n",
    );
    let (vault, _store) = vault_with_seeded_store(
        &[("projects/_parked/old.md", &body)],
        VaultConfig::default(),
    );

    let err = vault
        .add_action(dt(2026, 5, 1, 14, 0), "old", "anything", EnergyLevel::Deep)
        .unwrap_err();
    assert!(
        matches!(err, DomainError::ProjectNotActive(_)),
        "got {err:?}"
    );
}

#[test]
fn add_action_errors_when_project_not_found() {
    let (vault, _store) = vault_with_seeded_store(&[], VaultConfig::default());

    let err = vault
        .add_action(
            dt(2026, 5, 1, 14, 0),
            "ghost",
            "anything",
            EnergyLevel::Deep,
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            DomainError::Store(cdno_core::error::StoreError::NotFound(_))
        ),
        "got {err:?}"
    );
}

#[test]
fn add_action_errors_when_next_actions_section_missing() {
    let body = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# X\n\n## Current State\nFoo.\n";
    let (vault, _store) =
        vault_with_seeded_store(&[("projects/x.md", body)], VaultConfig::default());

    let err = vault
        .add_action(dt(2026, 5, 1, 14, 0), "x", "anything", EnergyLevel::Deep)
        .unwrap_err();
    assert!(matches!(err, DomainError::Manipulation(_)), "got {err:?}");
}

// ---------------------------------------------------------------------
// complete_action
// ---------------------------------------------------------------------

#[test]
fn complete_action_removes_matching_line_and_logs() {
    let body = project_body_with_actions(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "- [ ] Run feature set B (deep)\n- [ ] Draft methods (medium)\n",
    );
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .complete_action(dt(2026, 5, 1, 16, 30), "icml-paper", "feature set B")
        .expect("complete_action succeeds");

    let raw = store.read_file(&vp("projects/icml-paper.md")).unwrap();
    assert!(
        !raw.contains("Run feature set B"),
        "removed line still present:\n{raw}"
    );
    assert!(
        raw.contains("- [ ] Draft methods (medium)"),
        "other action lost:\n{raw}"
    );

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-01.md"))
        .expect("daily note exists");
    assert!(
        daily.contains("- **16:30**: action done on [[icml-paper]] — Run feature set B (deep)"),
        "missing log line:\n{daily}"
    );
}

#[test]
fn complete_action_matches_substring_case_insensitively() {
    let body = project_body_with_actions(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "- [ ] Run feature set B (deep)\n",
    );
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    // Lowercase substring of the action text — must match.
    vault
        .complete_action(dt(2026, 5, 1, 16, 0), "icml-paper", "feature")
        .expect("case-insensitive substring matches");

    let raw = store.read_file(&vp("projects/icml-paper.md")).unwrap();
    assert!(!raw.contains("Run feature set B"));
}

#[test]
fn complete_action_ignores_already_checked_lines() {
    let body = project_body_with_actions(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "- [x] Already done (light)\n- [ ] Still open (deep)\n",
    );
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    let err = vault
        .complete_action(dt(2026, 5, 1, 16, 0), "icml-paper", "Already done")
        .unwrap_err();
    assert!(
        matches!(err, DomainError::ActionNotFound { .. }),
        "closed lines must not match: got {err:?}"
    );

    // Now match the open one — the closed line must not appear in
    // candidate consideration (no ambiguity error from it).
    vault
        .complete_action(dt(2026, 5, 1, 16, 0), "icml-paper", "Still open")
        .expect("open line still matchable");

    let raw = store.read_file(&vp("projects/icml-paper.md")).unwrap();
    assert!(
        raw.contains("- [x] Already done (light)"),
        "closed line preserved:\n{raw}"
    );
}

#[test]
fn complete_action_errors_when_action_not_found() {
    let body = project_body_with_actions(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "- [ ] Existing (light)\n",
    );
    let (vault, _store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    let err = vault
        .complete_action(dt(2026, 5, 1, 16, 0), "icml-paper", "ghost")
        .unwrap_err();
    match err {
        DomainError::ActionNotFound { slug, query } => {
            assert_eq!(slug, "icml-paper");
            assert_eq!(query, "ghost");
        }
        other => panic!("expected ActionNotFound, got {other:?}"),
    }
}

#[test]
fn complete_action_errors_when_match_is_ambiguous() {
    let body = project_body_with_actions(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "- [ ] Run baseline ablation (deep)\n- [ ] Run sparse ablation (deep)\n",
    );
    let (vault, _store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    let err = vault
        .complete_action(dt(2026, 5, 1, 16, 0), "icml-paper", "ablation")
        .unwrap_err();
    match err {
        DomainError::AmbiguousAction {
            slug,
            query,
            candidates,
        } => {
            assert_eq!(slug, "icml-paper");
            assert_eq!(query, "ablation");
            assert_eq!(candidates.len(), 2);
            assert!(candidates.iter().any(|c| c.contains("baseline")));
            assert!(candidates.iter().any(|c| c.contains("sparse")));
        }
        other => panic!("expected AmbiguousAction, got {other:?}"),
    }
}

#[test]
fn complete_action_errors_when_project_parked() {
    let body = project_body_with_actions(
        "work",
        "parked",
        "2026-04-01",
        "Old",
        "- [ ] Anything (light)\n",
    );
    let (vault, _store) = vault_with_seeded_store(
        &[("projects/_parked/old.md", &body)],
        VaultConfig::default(),
    );

    let err = vault
        .complete_action(dt(2026, 5, 1, 16, 0), "old", "Anything")
        .unwrap_err();
    assert!(
        matches!(err, DomainError::ProjectNotActive(_)),
        "got {err:?}"
    );
}

#[test]
fn complete_action_handles_action_without_energy_suffix() {
    // Manually-edited project with a bullet that has no `(<energy>)`
    // tag — match should still work, and the log entry preserves the
    // text verbatim (no synthetic suffix).
    let body = project_body_with_actions(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "- [ ] Bare action with no energy\n",
    );
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .complete_action(dt(2026, 5, 1, 16, 0), "icml-paper", "Bare action")
        .expect("untagged actions are still matchable");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-01.md"))
        .unwrap();
    assert!(
        daily.contains("- **16:00**: action done on [[icml-paper]] — Bare action with no energy"),
        "log entry must preserve verbatim text:\n{daily}"
    );
}

#[test]
fn add_action_errors_when_status_mismatches_folder() {
    // Same defensive check as update_project_state — file lives at
    // projects/<slug>.md but frontmatter says parked. Frontmatter
    // wins; refuse the mutation.
    let body = project_body_with_actions(
        "work",
        "parked",
        "2026-04-01",
        "Mismatched",
        "- [ ] Step (light)\n",
    );
    let (vault, _store) =
        vault_with_seeded_store(&[("projects/mismatched.md", &body)], VaultConfig::default());

    let err = vault
        .add_action(dt(2026, 5, 1, 14, 0), "mismatched", "X", EnergyLevel::Deep)
        .unwrap_err();
    assert!(
        matches!(err, DomainError::ProjectNotActive(_)),
        "got {err:?}"
    );
}

#[test]
fn complete_action_preserves_other_sections() {
    let body = project_body_with_actions(
        "work",
        "active",
        "2026-04-01",
        "ICML paper",
        "- [ ] One (deep)\n- [ ] Two (light)\n",
    );
    let (vault, store) =
        vault_with_seeded_store(&[("projects/icml-paper.md", &body)], VaultConfig::default());

    vault
        .complete_action(dt(2026, 5, 1, 16, 0), "icml-paper", "One")
        .expect("complete_action succeeds");

    let raw = store.read_file(&vp("projects/icml-paper.md")).unwrap();
    assert!(raw.contains("## Current State"));
    assert!(raw.contains("Initial."));
    assert!(raw.contains("## Waiting On"));
    assert!(
        raw.contains("- [ ] Two (light)"),
        "other action preserved:\n{raw}"
    );
}
