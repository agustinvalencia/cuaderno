//! Unit tests for `Vault::create_commitment` and
//! `Vault::complete_commitment`. Uses `MemoryVaultStore` /
//! `MemoryIndex` so the suite stays fast and deterministic — no
//! disk I/O.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{CommitmentFrontmatter, CommitmentStatus, Context};
use cdno_domain::{CommitmentSource, Vault};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn vault_with_seeded_store(notes: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

fn read_commitment_frontmatter(
    store: &Arc<dyn VaultStore>,
    path: &VaultPath,
) -> CommitmentFrontmatter {
    let raw = store.read_file(path).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    CommitmentFrontmatter::try_from(fm).unwrap()
}

// ---------------------------------------------------------------------
// create_commitment
// ---------------------------------------------------------------------

#[test]
fn create_commitment_writes_file_with_active_status() {
    let (vault, store) = vault_with_seeded_store(&[]);

    let path = vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Renew passport",
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            Context::Personal,
            None,
            None,
        )
        .expect("create succeeds");

    assert_eq!(path, vp("commitments/renew-passport.md"));
    let fm = read_commitment_frontmatter(&store, &path);
    assert_eq!(fm.status, CommitmentStatus::Active);
    assert_eq!(fm.due, NaiveDate::from_ymd_opt(2026, 6, 30).unwrap());
    assert_eq!(fm.created, NaiveDate::from_ymd_opt(2026, 5, 2).unwrap());
    assert!(fm.completed.is_none(), "completed is null while active");
    assert_eq!(fm.context, Context::Personal);
    assert!(fm.project.is_none());
    assert!(fm.stewardship.is_none());
}

#[test]
fn create_commitment_persists_origin_links_as_bare_slugs() {
    let (vault, store) = vault_with_seeded_store(&[]);

    let path = vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Email ophthalmologist",
            NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            Context::Personal,
            Some("surrogate-model"),
            Some("health"),
        )
        .expect("create succeeds");

    let raw = store.read_file(&path).unwrap();
    // Bare slugs written as quoted YAML scalars, not wikilinks (see the
    // storage-form decision in #199).
    assert!(
        raw.contains("project: \"surrogate-model\""),
        "frontmatter:\n{raw}"
    );
    assert!(
        raw.contains("stewardship: \"health\""),
        "frontmatter:\n{raw}"
    );

    let fm = read_commitment_frontmatter(&store, &path);
    assert_eq!(fm.project.as_deref(), Some("surrogate-model"));
    assert_eq!(fm.stewardship.as_deref(), Some("health"));
}

#[test]
fn create_commitment_canonicalises_and_escapes_origin_links() {
    let (vault, store) = vault_with_seeded_store(&[]);

    // Mixed-case / spaced input is slugified to canonical form, and a
    // YAML-hostile value (a bare colon would otherwise break the
    // frontmatter) is neutralised by slugifying + quoting.
    let path = vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Email ophthalmologist",
            ymd(2026, 6, 15),
            Context::Personal,
            Some("Surrogate Model"),
            Some("a: b"),
        )
        .expect("create succeeds");

    // The file still parses as valid frontmatter despite the hostile
    // input, and the links round-trip as canonical slugs.
    let fm = read_commitment_frontmatter(&store, &path);
    assert_eq!(fm.project.as_deref(), Some("surrogate-model"));
    assert_eq!(fm.stewardship.as_deref(), Some("a-b"));

    // The canonical slug is exactly what the backlink query matches on.
    assert_eq!(
        vault
            .commitments_for_project("surrogate-model")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn create_commitment_normalises_blank_origin_links_to_null() {
    let (vault, store) = vault_with_seeded_store(&[]);

    let path = vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Renew passport",
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            Context::Personal,
            Some("   "),
            Some(""),
        )
        .expect("create succeeds");

    let fm = read_commitment_frontmatter(&store, &path);
    assert!(fm.project.is_none(), "blank project is dropped");
    assert!(fm.stewardship.is_none(), "blank stewardship is dropped");
}

#[test]
fn create_commitment_drops_links_with_no_alphanumerics_to_null() {
    let (vault, store) = vault_with_seeded_store(&[]);

    // An input with no alphanumerics slugifies to the "untitled"
    // sentinel; rather than write a phantom link to a non-existent
    // "untitled" target, it's dropped to null.
    let path = vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Renew passport",
            ymd(2026, 6, 30),
            Context::Personal,
            Some("!!!"),
            Some("---"),
        )
        .expect("create succeeds");

    let fm = read_commitment_frontmatter(&store, &path);
    assert!(fm.project.is_none(), "symbol-only project is dropped");
    assert!(
        fm.stewardship.is_none(),
        "symbol-only stewardship is dropped"
    );
}

#[test]
fn commitments_for_stewardship_returns_only_linked_commitments_sorted_by_due() {
    let (vault, _store) = vault_with_seeded_store(&[]);

    // Two linked to health, one linked to a different stewardship, one
    // standalone. Created out of due order to prove the sort.
    vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Eye exam booking",
            ymd(2026, 9, 1),
            Context::Personal,
            None,
            Some("health"),
        )
        .unwrap();
    vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Email ophthalmologist",
            ymd(2026, 6, 15),
            Context::Personal,
            None,
            Some("health"),
        )
        .unwrap();
    vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Renew home insurance",
            ymd(2026, 7, 1),
            Context::Personal,
            None,
            Some("finances"),
        )
        .unwrap();
    vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Renew passport",
            ymd(2026, 8, 1),
            Context::Personal,
            None,
            None,
        )
        .unwrap();

    let linked = vault.commitments_for_stewardship("health").unwrap();
    let slugs: Vec<&str> = linked
        .iter()
        .map(|(path, _)| path.as_path().file_stem().unwrap().to_str().unwrap())
        .collect();
    // Only the two health-linked commitments, earliest due first.
    assert_eq!(slugs, vec!["email-ophthalmologist", "eye-exam-booking"]);
    assert!(
        linked
            .iter()
            .all(|(_, c)| c.stewardship.as_deref() == Some("health"))
    );
}

#[test]
fn commitments_for_project_returns_only_project_linked_commitments() {
    let (vault, _store) = vault_with_seeded_store(&[]);

    vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Submit camera-ready",
            ymd(2026, 7, 10),
            Context::Work,
            Some("surrogate-model"),
            None,
        )
        .unwrap();
    vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Email ophthalmologist",
            ymd(2026, 6, 15),
            Context::Personal,
            None,
            Some("health"),
        )
        .unwrap();

    let linked = vault.commitments_for_project("surrogate-model").unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].1.project.as_deref(), Some("surrogate-model"));
    // The stewardship-only commitment is not a project match.
    assert!(vault.commitments_for_project("health").unwrap().is_empty());
}

#[test]
fn commitments_for_stewardship_includes_completed_commitments() {
    let (vault, _store) = vault_with_seeded_store(&[]);

    vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Email ophthalmologist",
            ymd(2026, 6, 15),
            Context::Personal,
            None,
            Some("health"),
        )
        .unwrap();
    // Completing the commitment moves it to _done/ and re-indexes it,
    // still typed `commitment`. The backlink view is a relationship
    // view, not a to-do list, so fulfilled commitments must still show.
    vault
        .complete_commitment(dt(2026, 6, 16, 9, 0), "email-ophthalmologist")
        .unwrap();

    let linked = vault.commitments_for_stewardship("health").unwrap();
    assert_eq!(linked.len(), 1, "completed commitment still surfaces");
    assert_eq!(linked[0].1.status, CommitmentStatus::Completed);
    assert!(
        linked[0].0.as_path().to_str().unwrap().contains("_done/"),
        "path: {:?}",
        linked[0].0
    );
}

#[test]
fn create_commitment_logs_creation_to_daily_note() {
    let (vault, store) = vault_with_seeded_store(&[]);

    vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Renew passport",
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            Context::Personal,
            None,
            None,
        )
        .expect("create succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-02.md"))
        .expect("daily note exists");
    assert!(
        daily.contains(
            "- **09:00**: commitment created [[renew-passport]] \u{2014} Renew passport (due 2026-06-30)"
        ),
        "log entry:\n{daily}"
    );
}

#[test]
fn create_commitment_errors_when_slug_collides() {
    let existing = "---\ntype: commitment\nstatus: active\ndue: 2026-06-30\ncreated: 2026-05-01\ncompleted: null\ncontext: personal\nproject: null\nstewardship: null\n---\n\n# Renew passport\n";
    let (vault, _store) = vault_with_seeded_store(&[("commitments/renew-passport.md", existing)]);

    let err = vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Renew passport",
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            Context::Personal,
            None,
            None,
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            DomainError::Store(cdno_core::error::StoreError::AlreadyExists(_))
        ),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------
// complete_commitment
// ---------------------------------------------------------------------

fn commitment_body(status: &str, due: &str, created: &str, completed: &str, title: &str) -> String {
    format!(
        "---\ntype: commitment\nstatus: {status}\ndue: {due}\ncreated: {created}\ncompleted: {completed}\ncontext: personal\nproject: null\nstewardship: null\n---\n\n# {title}\n"
    )
}

#[test]
fn complete_commitment_moves_file_and_stamps_completion() {
    let body = commitment_body(
        "active",
        "2026-06-30",
        "2026-05-01",
        "null",
        "Renew passport",
    );
    let (vault, store) = vault_with_seeded_store(&[("commitments/renew-passport.md", &body)]);

    let path = vault
        .complete_commitment(dt(2026, 5, 15, 14, 30), "renew-passport")
        .expect("complete succeeds");

    assert_eq!(path, vp("commitments/_done/2026/renew-passport.md"));
    assert!(
        !store.exists(&vp("commitments/renew-passport.md")).unwrap(),
        "active path emptied"
    );
    let fm = read_commitment_frontmatter(&store, &path);
    assert_eq!(fm.status, CommitmentStatus::Completed);
    assert_eq!(
        fm.completed,
        Some(NaiveDate::from_ymd_opt(2026, 5, 15).unwrap())
    );
}

#[test]
fn complete_commitment_logs_completion_to_daily_note() {
    let body = commitment_body(
        "active",
        "2026-06-30",
        "2026-05-01",
        "null",
        "Renew passport",
    );
    let (vault, store) = vault_with_seeded_store(&[("commitments/renew-passport.md", &body)]);

    vault
        .complete_commitment(dt(2026, 5, 15, 14, 30), "renew-passport")
        .expect("complete succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-15.md"))
        .expect("daily note exists");
    assert!(
        daily.contains(
            "- **14:30**: commitment completed [[renew-passport]] \u{2014} Renew passport"
        ),
        "log entry:\n{daily}"
    );
}

#[test]
fn complete_commitment_creates_year_subfolder_when_missing() {
    // Commitment created in 2026, completed in 2027 — the
    // `_done/2027/` directory doesn't exist yet because `cdno init`
    // only seeds the year of init. The store's write_file creates
    // parent dirs automatically.
    let body = commitment_body(
        "active",
        "2027-01-15",
        "2026-12-15",
        "null",
        "Year-crossing",
    );
    let (vault, store) = vault_with_seeded_store(&[("commitments/year-crossing.md", &body)]);

    let path = vault
        .complete_commitment(dt(2027, 1, 10, 9, 0), "year-crossing")
        .expect("complete succeeds across years");

    assert_eq!(path, vp("commitments/_done/2027/year-crossing.md"));
    assert!(store.exists(&path).unwrap());
}

#[test]
fn complete_commitment_errors_when_not_found() {
    let (vault, _store) = vault_with_seeded_store(&[]);

    let err = vault
        .complete_commitment(dt(2026, 5, 15, 9, 0), "ghost")
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
fn complete_commitment_errors_when_status_is_already_completed() {
    // Defensive: file at `commitments/<slug>.md` (active path) but
    // frontmatter says completed. Refuse rather than re-stamp.
    let body = commitment_body(
        "completed",
        "2026-06-30",
        "2026-05-01",
        "2026-05-10",
        "Drifted",
    );
    let (vault, _store) = vault_with_seeded_store(&[("commitments/drifted.md", &body)]);

    let err = vault
        .complete_commitment(dt(2026, 5, 15, 9, 0), "drifted")
        .unwrap_err();
    assert!(
        matches!(err, DomainError::CommitmentNotActive(_)),
        "got {err:?}"
    );
}

#[test]
fn complete_commitment_falls_back_to_slug_when_body_has_no_heading() {
    // Hand-edited commitment with no `# Title` line. The completion
    // log entry should fall back to the slug rather than crash or
    // emit empty text.
    let body = "---\ntype: commitment\nstatus: active\ndue: 2026-06-30\ncreated: 2026-05-01\ncompleted: null\ncontext: personal\nproject: null\nstewardship: null\n---\n\nNo heading at all, just body text.\n";
    let (vault, store) = vault_with_seeded_store(&[("commitments/headless.md", body)]);

    vault
        .complete_commitment(dt(2026, 5, 15, 9, 0), "headless")
        .expect("complete succeeds even without a body heading");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-15.md"))
        .unwrap();
    assert!(
        daily.contains("- **09:00**: commitment completed [[headless]] \u{2014} headless"),
        "log falls back to slug:\n{daily}"
    );
}

#[test]
fn complete_commitment_errors_when_destination_already_exists() {
    // Drift scenario: an active commitment and an already-completed
    // copy share a slug for the completion year. Refuse rather than
    // overwriting.
    let active = commitment_body("active", "2026-06-30", "2026-05-01", "null", "Same");
    let already_done = commitment_body(
        "completed",
        "2026-06-30",
        "2026-04-01",
        "2026-05-10",
        "Same (older)",
    );
    let (vault, _store) = vault_with_seeded_store(&[
        ("commitments/same.md", &active),
        ("commitments/_done/2026/same.md", &already_done),
    ]);

    let err = vault
        .complete_commitment(dt(2026, 5, 15, 9, 0), "same")
        .unwrap_err();
    assert!(
        matches!(
            err,
            DomainError::Store(cdno_core::error::StoreError::AlreadyExists(_))
        ),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------
// commitments aggregation (#32)
// ---------------------------------------------------------------------

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

/// Active project whose `## Milestones` mixes a hard deadline (a
/// commitment), a soft target, and a completed marker (the latter two
/// excluded from the aggregation).
const PROJECT_WITH_MILESTONES: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Milestones\n- [ ] Submit paper — hard: 2026-06-01\n- [ ] Polish — target: 2026-06-02\n- [x] Kickoff — 2026-05-01\n\n## Next Actions\n";

fn agg_commitment_note(due: &str, status: &str) -> String {
    format!(
        "---\ntype: commitment\nstatus: {status}\ndue: {due}\ncreated: 2026-05-01\ncompleted: null\ncontext: personal\n---\n\n# Renew passport\n"
    )
}

fn agg_action_note(title: &str, due: &str, milestone: &str) -> String {
    format!(
        "---\ntype: action\nstatus: active\nproject: alpha\nenergy: deep\nmilestone: {milestone}\ndue: {due}\ncreated: 2026-05-20\ncompleted: null\nblocker: null\ncriteria: null\ntags: []\n---\n\n# {title}\n"
    )
}

#[test]
fn commitments_aggregates_all_sources_sorted_by_date() {
    let (vault, _store) = vault_with_seeded_store(&[
        ("projects/alpha.md", PROJECT_WITH_MILESTONES),
        (
            "commitments/renew-passport.md",
            &agg_commitment_note("2026-05-30", "active"),
        ),
        (
            "actions/write-draft.md",
            &agg_action_note("Write the draft", "2026-05-28", "null"),
        ),
        // Milestone-pinned action: covered by its milestone (source 1),
        // must not be duplicated here.
        (
            "actions/pinned-work.md",
            &agg_action_note(
                "Pinned work",
                "2026-05-29",
                "\"[[projects/alpha#submit-paper]]\"",
            ),
        ),
    ]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();

    let summary: Vec<(NaiveDate, &str, &CommitmentSource)> = got
        .iter()
        .map(|c| (c.date, c.title.as_str(), &c.source))
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                ymd(2026, 5, 28),
                "Write the draft",
                &CommitmentSource::ActionNote("alpha".to_owned()),
            ),
            (
                ymd(2026, 5, 30),
                "Renew passport",
                &CommitmentSource::StandaloneCommitment,
            ),
            (
                ymd(2026, 6, 1),
                "Submit paper",
                &CommitmentSource::ProjectMilestone("alpha".to_owned()),
            ),
        ],
    );
    assert!(
        got.iter().all(|c| !c.is_overdue),
        "all dates are in the future"
    );
}

#[test]
fn commitments_flags_overdue_within_lookback_and_excludes_beyond_window() {
    let (vault, _store) = vault_with_seeded_store(&[
        // 6 days before today — overdue but inside the 30-day look-back.
        (
            "commitments/recent.md",
            &agg_commitment_note("2026-05-20", "active"),
        ),
        // 36 days before today — past the look-back, excluded.
        (
            "commitments/ancient.md",
            &agg_commitment_note("2026-04-20", "active"),
        ),
        // Past the lookahead window, excluded.
        (
            "commitments/distant.md",
            &agg_commitment_note("2026-07-01", "active"),
        ),
        // Completed, excluded regardless of date.
        (
            "commitments/done.md",
            &agg_commitment_note("2026-05-28", "completed"),
        ),
    ]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    assert_eq!(got.len(), 1, "only the recent overdue commitment: {got:?}");
    assert_eq!(got[0].date, ymd(2026, 5, 20));
    assert!(got[0].is_overdue);
}

#[test]
fn commitments_does_not_duplicate_a_milestone_pinned_action() {
    // A project hard milestone plus an action pinned to it: the
    // milestone is the single source of truth, so exactly one entry.
    let (vault, _store) = vault_with_seeded_store(&[
        ("projects/alpha.md", PROJECT_WITH_MILESTONES),
        (
            "actions/pinned-work.md",
            &agg_action_note(
                "Pinned work",
                "2026-05-28",
                "\"[[projects/alpha#submit-paper]]\"",
            ),
        ),
    ]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    assert_eq!(
        got.len(),
        1,
        "milestone only, action not duplicated: {got:?}"
    );
    assert_eq!(
        got[0].source,
        CommitmentSource::ProjectMilestone("alpha".to_owned()),
    );
    assert_eq!(got[0].title, "Submit paper");
}

// ---------------------------------------------------------------------
// Source 2: stewardship periodic commitments
// ---------------------------------------------------------------------

/// Stewardship body shaped like the design §5.6 expanded example,
/// with a `## Periodic Commitments` section pre-populated. The
/// aggregation parses each `- title — recurrence — next: YYYY-MM-DD`
/// line.
fn stewardship_with_periodics(slug: &str, lines: &str) -> String {
    let context = "personal";
    format!(
        "---\ntype: stewardship\ncontext: {context}\n---\n\n# {slug}\n\n## Current Status\nN/A.\n\n## Periodic Commitments\n{lines}"
    )
}

#[test]
fn commitments_surfaces_periodic_lines_from_a_flat_stewardship() {
    let lines = "- Tax declaration \u{2014} yearly \u{2014} next: 2026-06-01\n- Budget review \u{2014} monthly \u{2014} next: 2026-05-30\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/finances.md",
        &stewardship_with_periodics("Finances", lines),
    )]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    let summary: Vec<(NaiveDate, &str, &CommitmentSource)> = got
        .iter()
        .map(|c| (c.date, c.title.as_str(), &c.source))
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                ymd(2026, 5, 30),
                "Budget review",
                &CommitmentSource::Stewardship("finances".to_owned()),
            ),
            (
                ymd(2026, 6, 1),
                "Tax declaration",
                &CommitmentSource::Stewardship("finances".to_owned()),
            ),
        ],
    );
}

#[test]
fn commitments_surfaces_periodic_lines_from_an_expanded_stewardship() {
    let lines = "- Dental check-up \u{2014} every 6 months \u{2014} next: 2026-05-28\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/health/_index.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(
        got[0].source,
        CommitmentSource::Stewardship("health".to_owned())
    );
    assert_eq!(got[0].title, "Dental check-up");
    assert_eq!(got[0].date, ymd(2026, 5, 28));
}

#[test]
fn commitments_flags_overdue_periodic_within_lookback_and_excludes_outside_window() {
    let lines = "- Recent overdue \u{2014} monthly \u{2014} next: 2026-05-20\n- Ancient overdue \u{2014} monthly \u{2014} next: 2026-04-20\n- Distant future \u{2014} yearly \u{2014} next: 2026-07-01\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/finances.md",
        &stewardship_with_periodics("Finances", lines),
    )]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    assert_eq!(got.len(), 1, "only the recent overdue periodic: {got:?}");
    assert_eq!(got[0].title, "Recent overdue");
    assert!(got[0].is_overdue);
}

#[test]
fn commitments_tolerates_overdue_annotation_and_skips_malformed_periodic_lines() {
    let lines = "- Dental check-up \u{2014} every 6 months \u{2014} next: 2026-05-28 (overdue)\n- Garbage line without separators\n- \u{2014} \u{2014} next: not-a-date\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/health/_index.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    assert_eq!(
        got.len(),
        1,
        "only the well-formed periodic line surfaces: {got:?}"
    );
    assert_eq!(got[0].title, "Dental check-up");
}

#[test]
fn complete_commitment_not_found_lists_open_commitments_excluding_done() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_commitment(
            dt(2026, 1, 10, 9, 0),
            "Submit paper",
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            Context::Work,
            None,
            None,
        )
        .unwrap();
    vault
        .create_commitment(
            dt(2026, 1, 10, 9, 0),
            "Renew passport",
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
            Context::Personal,
            None,
            None,
        )
        .unwrap();

    // Both open → both listed, slug-sorted.
    let err = vault
        .complete_commitment(dt(2026, 2, 1, 9, 0), "missing")
        .unwrap_err();
    let DomainError::Store(StoreError::NotFound(msg)) = err else {
        panic!("expected Store(NotFound), got {err:?}");
    };
    assert!(
        msg.ends_with("available commitments: renew-passport, submit-paper"),
        "got: {msg}"
    );

    // Fulfil one — it moves under commitments/_done/ and must drop out.
    vault
        .complete_commitment(dt(2026, 2, 1, 9, 0), "submit-paper")
        .unwrap();
    let err = vault
        .complete_commitment(dt(2026, 2, 2, 9, 0), "missing")
        .unwrap_err();
    let DomainError::Store(StoreError::NotFound(msg)) = err else {
        panic!("expected Store(NotFound), got {err:?}");
    };
    assert!(
        msg.ends_with("available commitments: renew-passport"),
        "done commitment must be excluded, got: {msg}"
    );
}

// ---------------------------------------------------------------------
// CommitmentSource JSON shape (#210 review)
// ---------------------------------------------------------------------

#[test]
fn commitment_source_serializes_with_a_homogeneous_kind_tag() {
    use cdno_domain::frontmatter::Context;
    use cdno_domain::{CommitmentEntry, CommitmentSource};
    use chrono::NaiveDate;

    let date = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
    let tuple = CommitmentEntry {
        date,
        title: "Ship v1".to_owned(),
        source: CommitmentSource::ProjectMilestone("surrogate".to_owned()),
        is_overdue: false,
        context: Context::Work,
    };
    let unit = CommitmentEntry {
        date,
        title: "A promise".to_owned(),
        source: CommitmentSource::StandaloneCommitment,
        is_overdue: false,
        context: Context::Personal,
    };

    // Tuple variant: {"kind":"project_milestone","slug":"surrogate"}.
    let tuple_json = serde_json::to_value(&tuple).unwrap();
    assert_eq!(tuple_json["source"]["kind"], "project_milestone");
    assert_eq!(tuple_json["source"]["slug"], "surrogate");

    // Unit variant: {"kind":"standalone_commitment"} -- same `kind`
    // tag, no `slug` (homogeneous, not a bare string).
    let unit_json = serde_json::to_value(&unit).unwrap();
    assert_eq!(unit_json["source"]["kind"], "standalone_commitment");
    assert!(
        unit_json["source"].get("slug").is_none(),
        "unit variant carries no slug: {unit_json}"
    );
}
