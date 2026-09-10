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
fn create_commitment_suffixes_when_slug_collides() {
    // #225: a second same-title commitment suffixes to `-2` (so a later move
    // to `_done/` keeps its backlinks resolvable) rather than erroring.
    let existing = "---\ntype: commitment\nstatus: active\ndue: 2026-06-30\ncreated: 2026-05-01\ncompleted: null\ncontext: personal\nproject: null\nstewardship: null\n---\n\n# Renew passport\n";
    let (vault, _store) = vault_with_seeded_store(&[("commitments/renew-passport.md", existing)]);

    let path = vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Renew passport",
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            Context::Personal,
            None,
            None,
        )
        .expect("colliding commitment now suffixes");
    assert_eq!(path, vp("commitments/renew-passport-2.md"));
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
                &CommitmentSource::StandaloneCommitment("renew-passport".to_owned()),
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

/// The em dash is the separator the grammar itself chose, and the vault's
/// prose style uses it constantly — so a title carrying one is the natural
/// way to write these lines, not an exotic case (#453). Under the old
/// left-anchored split the third segment stopped being the `next:` part and
/// the commitment disappeared from the register entirely. Note the title here
/// contains *two* segments, so the line carries three em dashes in total.
#[test]
fn commitments_surfaces_a_periodic_line_whose_title_contains_an_em_dash() {
    let lines = "- Monthly review \u{2014} check the account balance \u{2014} monthly \u{2014} next: 2026-05-29\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/finances.md",
        &stewardship_with_periodics("Finances", lines),
    )]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    assert_eq!(got.len(), 1, "the line must reach the register: {got:?}");
    assert_eq!(
        got[0].title,
        "Monthly review \u{2014} check the account balance"
    );
    assert_eq!(got[0].date, ymd(2026, 5, 29));
    assert_eq!(
        got[0].source,
        CommitmentSource::Stewardship("finances".to_owned())
    );
}

/// The other half of the same problem, and the reason the parse anchors on
/// the marker instead of counting dashes from either end. The parser tolerates
/// a trailing annotation after the date so hand-annotated lines round-trip, and
/// an annotation is the freest prose on the line — so it attracts em dashes
/// exactly as titles do. Counting from the right would drop these, which is
/// #453 again with the victims swapped.
#[test]
fn commitments_surfaces_a_periodic_line_annotated_with_an_em_dash_after_the_date() {
    let lines = "- Dental check-up \u{2014} every 6 months \u{2014} next: 2026-05-28 (overdue \u{2014} rebook)\n- Eye exam \u{2014} yearly \u{2014} next: 2026-05-30 \u{2014} moved from April\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/health/_index.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    let summary: Vec<(&str, NaiveDate)> = got.iter().map(|c| (c.title.as_str(), c.date)).collect();
    assert_eq!(
        summary,
        vec![
            ("Dental check-up", ymd(2026, 5, 28)),
            ("Eye exam", ymd(2026, 5, 30)),
        ],
        "an em dash in the annotation must not evict the line",
    );
}

/// A `next:` inside a trailing annotation must not steal the anchor from the
/// real marker — the first candidate wins, so the date read is the one the
/// grammar puts there and not whatever the prose mentions afterwards.
#[test]
fn commitments_ignores_a_second_next_marker_inside_the_annotation() {
    let lines = "- Boiler service \u{2014} yearly \u{2014} next: 2026-05-28 \u{2014} was next: 2026-04-01\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/home.md",
        &stewardship_with_periodics("Home", lines),
    )]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].title, "Boiler service");
    assert_eq!(got[0].date, ymd(2026, 5, 28));
}

/// The one ambiguity the marker anchor does not remove: the recurrence is the
/// segment immediately before the marker, so an em dash in a *recurrence* is
/// absorbed into the title. Pinned deliberately — it is the documented
/// direction to be wrong in, and a change of mind should have to edit this
/// assertion rather than discover it in a vault.
#[test]
fn commitments_reads_an_em_dash_in_the_recurrence_as_part_of_the_title() {
    let lines = "- Deep clean \u{2014} every 3 \u{2014} 4 months \u{2014} next: 2026-05-29\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/home.md",
        &stewardship_with_periodics("Home", lines),
    )]);

    let got = vault.commitments(ymd(2026, 5, 26), 14).unwrap();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].title, "Deep clean \u{2014} every 3");
    assert_eq!(got[0].date, ymd(2026, 5, 29));
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
    let standalone = CommitmentEntry {
        date,
        title: "A promise".to_owned(),
        source: CommitmentSource::StandaloneCommitment("a-promise".to_owned()),
        is_overdue: false,
        context: Context::Personal,
    };

    // Tuple variant: {"kind":"project_milestone","slug":"surrogate"}.
    let tuple_json = serde_json::to_value(&tuple).unwrap();
    assert_eq!(tuple_json["source"]["kind"], "project_milestone");
    assert_eq!(tuple_json["source"]["slug"], "surrogate");

    // Standalone now carries its own slug so a consumer can complete it
    // directly: {"kind":"standalone_commitment","slug":"a-promise"} —
    // still the homogeneous `kind`+`slug` shape, not a bare string.
    let standalone_json = serde_json::to_value(&standalone).unwrap();
    assert_eq!(standalone_json["source"]["kind"], "standalone_commitment");
    assert_eq!(standalone_json["source"]["slug"], "a-promise");
}

// ---- reschedule_commitment (#430) ----

/// The point of the verb, and the reason delete-and-recreate was not
/// good enough: the body and `created` survive, and the move is on the
/// record with both dates.
#[test]
fn reschedule_commitment_moves_the_date_and_logs_both() {
    let note = "---\ntype: commitment\ncontext: work\nstatus: active\ncreated: 2026-04-01\ndue: 2026-06-01\n---\n\n# Quarterly report\n\nChased Bob twice; he is waiting on finance.\n";
    let (vault, store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", note)]);

    vault
        .reschedule_commitment(
            dt(2026, 5, 20, 9, 30),
            "quarterly-report",
            NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
        )
        .expect("reschedule succeeds");

    let raw = store
        .read_file(&vp("commitments/quarterly-report.md"))
        .unwrap();
    assert!(raw.contains("due: 2026-06-15"), "date moved:\n{raw}");
    assert!(
        raw.contains("created: 2026-04-01"),
        "created is not reset:\n{raw}"
    );
    assert!(
        raw.contains("Chased Bob twice"),
        "the body survives, which delete-and-recreate destroyed:\n{raw}"
    );

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-20.md"))
        .unwrap();
    assert!(
        daily.contains("- **09:30**: commitment rescheduled on [[quarterly-report]] \u{2014} Quarterly report\n  was: 2026-06-01\n  now: 2026-06-15"),
        "both dates on the record, in the was/now shape:\n{daily}"
    );
}

/// Slippage is the signal worth seeing, so a commitment can move more
/// than once and each move stands on its own in the log.
#[test]
fn reschedule_commitment_records_every_move_separately() {
    let note = "---\ntype: commitment\ncontext: work\nstatus: active\ncreated: 2026-04-01\ndue: 2026-06-01\n---\n\n# Quarterly report\n";
    let (vault, store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", note)]);

    for (day, to) in [(20, (2026, 6, 15)), (21, (2026, 6, 30))] {
        vault
            .reschedule_commitment(
                dt(2026, 5, day, 9, 30),
                "quarterly-report",
                NaiveDate::from_ymd_opt(to.0, to.1, to.2).unwrap(),
            )
            .expect("reschedule succeeds");
    }

    let second = store
        .read_file(&vp("journal/2026/daily/2026-05-21.md"))
        .unwrap();
    assert!(
        second.contains("was: 2026-06-15\n  now: 2026-06-30"),
        "the second move starts where the first left off:\n{second}"
    );
}

/// Moving to the date it already has would log a slip that never
/// happened, so it is refused rather than written as a no-op.
#[test]
fn reschedule_commitment_refuses_an_unchanged_date_and_writes_nothing() {
    let note = "---\ntype: commitment\ncontext: work\nstatus: active\ncreated: 2026-04-01\ndue: 2026-06-01\n---\n\n# Quarterly report\n";
    let (vault, store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", note)]);

    let err = vault
        .reschedule_commitment(
            dt(2026, 5, 20, 9, 30),
            "quarterly-report",
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
        )
        .expect_err("an unchanged date is not a reschedule");
    assert!(
        matches!(err, DomainError::CommitmentAlreadyDue { .. }),
        "got {err:?}"
    );
    assert!(
        !store
            .exists(&vp("journal/2026/daily/2026-05-20.md"))
            .unwrap(),
        "a rejected call logs nothing"
    );
}

/// Pulling a date earlier is as real a change as pushing it back.
#[test]
fn reschedule_commitment_allows_moving_a_date_earlier() {
    let note = "---\ntype: commitment\ncontext: work\nstatus: active\ncreated: 2026-04-01\ndue: 2026-06-01\n---\n\n# Quarterly report\n";
    let (vault, store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", note)]);

    vault
        .reschedule_commitment(
            dt(2026, 5, 20, 9, 30),
            "quarterly-report",
            NaiveDate::from_ymd_opt(2026, 5, 25).unwrap(),
        )
        .expect("earlier is a legitimate move");

    let raw = store
        .read_file(&vp("commitments/quarterly-report.md"))
        .unwrap();
    assert!(raw.contains("due: 2026-05-25"), "{raw}");
}

/// A fulfilled commitment lives in `_done/` and has no date left to
/// move; the slug no longer resolves to an active note.
#[test]
fn reschedule_commitment_errors_on_a_completed_commitment() {
    let note = "---\ntype: commitment\ncontext: work\nstatus: completed\ncreated: 2026-04-01\ndue: 2026-06-01\ncompleted: 2026-05-02\n---\n\n# Quarterly report\n";
    let (vault, _store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", note)]);

    let err = vault
        .reschedule_commitment(
            dt(2026, 5, 20, 9, 30),
            "quarterly-report",
            NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
        )
        .expect_err("a completed commitment cannot be rescheduled");
    assert!(
        matches!(err, DomainError::CommitmentNotActive(_)),
        "got {err:?}"
    );
}

// ---- complete_periodic (#558) ----

/// The gap #558 reports: nothing could mark a periodic commitment done,
/// so the reminder fired for ever until someone hand-edited the file.
#[test]
fn complete_periodic_rolls_the_date_forward_by_its_own_recurrence() {
    let lines = "- Dental check-up \u{2014} every 6 months \u{2014} next: 2026-09-01\n- Budget review \u{2014} monthly \u{2014} next: 2026-05-30\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/health.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    vault
        .complete_periodic(dt(2026, 9, 1, 9, 30), "health", "dental")
        .expect("complete succeeds");

    let raw = store.read_file(&vp("stewardships/health.md")).unwrap();
    assert!(
        raw.contains("- Dental check-up \u{2014} every 6 months \u{2014} next: 2027-03-01"),
        "six months on, day preserved:\n{raw}"
    );
    assert!(
        raw.contains("- Budget review \u{2014} monthly \u{2014} next: 2026-05-30"),
        "the sibling line is untouched:\n{raw}"
    );

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-09-01.md"))
        .unwrap();
    assert!(
        daily.contains("- **09:30**: periodic done on [[health]] \u{2014} Dental check-up\n  was: 2026-09-01\n  now: 2027-03-01"),
        "the completion and the new schedule are both on the record:\n{daily}"
    );
}

/// The anchoring rule, and the reason it matters. Done a week early,
/// every cycle, the schedule must not creep a week earlier each time.
#[test]
fn complete_periodic_anchors_the_roll_forward_to_the_due_date_not_the_completion() {
    let lines = "- Dental check-up \u{2014} every 6 months \u{2014} next: 2026-09-01\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/health.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    // Done a week early.
    vault
        .complete_periodic(dt(2026, 8, 25, 9, 30), "health", "dental")
        .expect("complete succeeds");

    let raw = store.read_file(&vp("stewardships/health.md")).unwrap();
    assert!(
        raw.contains("next: 2027-03-01"),
        "anchored to the due date (2026-09-01 + 6 months), not to 2026-08-25 \
         which would give 2027-02-25 and drift a week earlier every cycle:\n{raw}"
    );
}

/// A late completion must not leave `next:` in the past — that would
/// report the commitment overdue the instant it was done.
#[test]
fn complete_periodic_advances_past_a_long_neglect_in_one_go() {
    let lines = "- Budget review \u{2014} monthly \u{2014} next: 2026-01-15\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/finances.md",
        &stewardship_with_periodics("Finances", lines),
    )]);

    // Five cycles missed.
    vault
        .complete_periodic(dt(2026, 6, 20, 9, 30), "finances", "budget")
        .expect("complete succeeds");

    let raw = store.read_file(&vp("stewardships/finances.md")).unwrap();
    assert!(
        raw.contains("next: 2026-07-15"),
        "back on schedule and in the future, not five reminders deep:\n{raw}"
    );
}

/// The date is rewritten from the right, so a title carrying the *same*
/// date-shaped text is not the one that moves. The title must repeat the
/// marker's date for this to distinguish anything: a title holding some
/// other date leaves one occurrence on the line, which left-to-right and
/// right-to-left search find alike.
#[test]
fn complete_periodic_rewrites_the_marker_date_not_one_inside_the_title() {
    let lines = "- Review 2026-09-01 minutes \u{2014} yearly \u{2014} next: 2026-09-01\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/admin.md",
        &stewardship_with_periodics("Admin", lines),
    )]);

    vault
        .complete_periodic(dt(2026, 9, 1, 9, 30), "admin", "minutes")
        .expect("complete succeeds");

    let raw = store.read_file(&vp("stewardships/admin.md")).unwrap();
    assert!(
        raw.contains("- Review 2026-09-01 minutes \u{2014} yearly \u{2014} next: 2027-09-01"),
        "the title's date is untouched:\n{raw}"
    );
}

/// A recurrence the parser cannot read cannot be rolled forward, and
/// guessing a schedule is worse than refusing.
#[test]
fn complete_periodic_refuses_a_line_whose_recurrence_is_unreadable() {
    let lines = "- Dental check-up \u{2014} twice a year \u{2014} next: 2026-09-01\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/health.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    let err = vault
        .complete_periodic(dt(2026, 9, 1, 9, 30), "health", "dental")
        .expect_err("an unreadable recurrence cannot be advanced");
    assert!(
        matches!(err, DomainError::PeriodicRecurrenceUnreadable { .. }),
        "got {err:?}"
    );

    let raw = store.read_file(&vp("stewardships/health.md")).unwrap();
    assert!(
        raw.contains("next: 2026-09-01"),
        "nothing is written on the error path:\n{raw}"
    );
}

/// Such a line must still parse everywhere else. Making the recurrence
/// mandatory would have dropped it from `cdno commitments` — a silent
/// regression in a view the weekly review depends on.
#[test]
fn a_line_with_an_unreadable_recurrence_still_reaches_the_commitments_view() {
    let lines = "- Dental check-up \u{2014} twice a year \u{2014} next: 2026-09-01\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/health.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    let items = vault
        .commitments(NaiveDate::from_ymd_opt(2026, 8, 20).unwrap(), 30)
        .expect("commitments");
    assert!(
        items.iter().any(|c| c.title.contains("Dental check-up")),
        "the aggregation must be unaffected by #558's parser change: {items:?}"
    );
}

#[test]
fn complete_periodic_refuses_an_ambiguous_title_and_writes_nothing() {
    let lines = "- Budget review \u{2014} monthly \u{2014} next: 2026-05-30\n- Budget forecast \u{2014} yearly \u{2014} next: 2026-06-30\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/finances.md",
        &stewardship_with_periodics("Finances", lines),
    )]);

    let err = vault
        .complete_periodic(dt(2026, 5, 30, 9, 30), "finances", "budget")
        .expect_err("two matches must not be resolved by guessing");
    match err {
        DomainError::AmbiguousPeriodic { candidates, .. } => {
            assert_eq!(
                candidates.len(),
                2,
                "both candidates offered: {candidates:?}"
            );
        }
        other => panic!("got {other:?}"),
    }
    let raw = store.read_file(&vp("stewardships/finances.md")).unwrap();
    assert!(raw.contains("next: 2026-05-30") && raw.contains("next: 2026-06-30"));
}

#[test]
fn complete_periodic_errors_when_no_line_matches() {
    let lines = "- Budget review \u{2014} monthly \u{2014} next: 2026-05-30\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/finances.md",
        &stewardship_with_periodics("Finances", lines),
    )]);

    let err = vault
        .complete_periodic(dt(2026, 5, 30, 9, 30), "finances", "dental")
        .expect_err("no match");
    assert!(
        matches!(err, DomainError::PeriodicNotFound { .. }),
        "got {err:?}"
    );
}

/// The parser reads `%Y-%m-%d` loosely, so `next: 2026-9-1` is a valid
/// 2026-09-01 that the line does not spell that way. Rewriting by
/// searching for the *formatted* old date found nothing and silently
/// returned the line unchanged — while the caller went on to log a move.
/// The daily log is the vault's record of what happened; it must not
/// assert a change absent from the file it describes.
#[test]
fn complete_periodic_moves_a_date_the_line_does_not_zero_pad() {
    let lines = "- Dental check-up \u{2014} monthly \u{2014} next: 2026-9-1\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/health.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    vault
        .complete_periodic(dt(2026, 9, 1, 9, 30), "health", "dental")
        .expect("complete succeeds");

    let raw = store.read_file(&vp("stewardships/health.md")).unwrap();
    assert!(
        raw.contains("next: 2026-10-01"),
        "the date must actually move, not just be logged as moved:\n{raw}"
    );
    assert!(!raw.contains("2026-9-1"), "the old date is gone:\n{raw}");
}

/// A trailing annotation may repeat the marker's date. Anchoring the
/// rewrite at the last *date* rewrote the annotation and left the
/// schedule alone; anchoring at the `next:` marker cannot.
#[test]
fn complete_periodic_rewrites_the_marker_not_a_trailing_annotation() {
    let lines = "- Renew passport \u{2014} yearly \u{2014} next: 2026-09-01 (booked 2026-09-01)\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/admin.md",
        &stewardship_with_periodics("Admin", lines),
    )]);

    vault
        .complete_periodic(dt(2026, 9, 1, 9, 30), "admin", "passport")
        .expect("complete succeeds");

    let raw = store.read_file(&vp("stewardships/admin.md")).unwrap();
    assert!(
        raw.contains("next: 2027-09-01 (booked 2026-09-01)"),
        "the schedule moves and the annotation is left as written:\n{raw}"
    );
}

/// Counting cycles from the anchor rather than stepping one at a time.
/// Stepping compounds the day clamp: 31 Jan becomes 28 Feb, and stepping
/// *that* gives 28 Mar, losing the 31st for good. Counting re-derives
/// each occurrence from the same day.
#[test]
fn complete_periodic_keeps_the_anchor_day_across_several_missed_cycles() {
    let lines = "- Pay rent \u{2014} monthly \u{2014} next: 2026-01-31\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/finances.md",
        &stewardship_with_periodics("Finances", lines),
    )]);

    // Three cycles missed: Feb, Mar, Apr.
    vault
        .complete_periodic(dt(2026, 4, 15, 9, 30), "finances", "rent")
        .expect("complete succeeds");

    let raw = store.read_file(&vp("stewardships/finances.md")).unwrap();
    assert!(
        raw.contains("next: 2026-04-30"),
        "April is the first month landing after 15 April, clamped to its \
         own length from the 31st anchor — not 2026-04-28, which is what \
         compounding February's clamp gives:\n{raw}"
    );
}

/// A line whose `next:` value is not a date cannot be advanced. Refusing
/// is the point: the alternative was writing the file unchanged and
/// logging a move anyway.
#[test]
fn complete_periodic_refuses_a_line_whose_next_value_is_not_a_date() {
    // Parses as a periodic line (the parser takes the first token after
    // the marker) but the token is not a date, so nothing is rewritable.
    let lines = "- Dental check-up \u{2014} monthly \u{2014} next: soon\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/health.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    let err = vault
        .complete_periodic(dt(2026, 9, 1, 9, 30), "health", "dental")
        .expect_err("an unparseable next: cannot be advanced");
    // The parser rejects it outright, so it never resolves to a match.
    assert!(
        matches!(err, DomainError::PeriodicNotFound { .. }),
        "got {err:?}"
    );
    let raw = store.read_file(&vp("stewardships/health.md")).unwrap();
    assert!(raw.contains("next: soon"), "unchanged:\n{raw}");
}

/// Every title contains the empty string, so an empty query would match
/// the whole section and silently complete whichever line was alone.
#[test]
fn complete_periodic_refuses_an_empty_title() {
    let lines = "- Dental check-up \u{2014} monthly \u{2014} next: 2026-09-01\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/health.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    let err = vault
        .complete_periodic(dt(2026, 9, 1, 9, 30), "health", "   ")
        .expect_err("an empty query is not a match for everything");
    assert!(
        matches!(err, DomainError::EmptyField { field: "title" }),
        "got {err:?}"
    );
    let raw = store.read_file(&vp("stewardships/health.md")).unwrap();
    assert!(raw.contains("next: 2026-09-01"), "unchanged:\n{raw}");
}

/// The writer must locate the marker the way the parser did, or the two
/// disagree on exactly the lines where it matters. A trailing annotation
/// mentioning `next:` steals a right-anchored search...
#[test]
fn complete_periodic_is_not_fooled_by_an_annotation_mentioning_the_marker() {
    let lines = "- Dental check-up \u{2014} monthly \u{2014} next: 2026-09-01 (next: confirm with clinic)\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/health.md",
        &stewardship_with_periodics("Health", lines),
    )]);

    vault
        .complete_periodic(dt(2026, 9, 1, 9, 30), "health", "dental")
        .expect("complete succeeds");

    let raw = store.read_file(&vp("stewardships/health.md")).unwrap();
    assert!(
        raw.contains("next: 2026-10-01 (next: confirm with clinic)"),
        "the schedule moves; the annotation is left alone:\n{raw}"
    );
}

/// ...and a title mentioning `next:` steals a left-anchored one.
#[test]
fn complete_periodic_is_not_fooled_by_a_title_mentioning_the_marker() {
    let lines = "- Plan next: quarter \u{2014} monthly \u{2014} next: 2026-09-01\n";
    let (vault, store) = vault_with_seeded_store(&[(
        "stewardships/admin.md",
        &stewardship_with_periodics("Admin", lines),
    )]);

    vault
        .complete_periodic(dt(2026, 9, 1, 9, 30), "admin", "quarter")
        .expect("complete succeeds");

    let raw = store.read_file(&vp("stewardships/admin.md")).unwrap();
    assert!(
        raw.contains("- Plan next: quarter \u{2014} monthly \u{2014} next: 2026-10-01"),
        "the title is untouched and the schedule moves:\n{raw}"
    );
}

// ---- drop_commitment (#573) ----

const ACTIVE_COMMITMENT: &str = "---\ntype: commitment\nstatus: active\ndue: 2026-06-01\ncreated: 2026-04-01\ncompleted: null\ncontext: work\n---\n\n# Quarterly report\n\nPromised to Bob in the April review.\n";

/// The verb's reason for existing: a cancelled promise gets an ending
/// that does not claim it was kept.
#[test]
fn drop_commitment_archives_as_dropped_not_completed() {
    let (vault, store) =
        vault_with_seeded_store(&[("commitments/quarterly-report.md", ACTIVE_COMMITMENT)]);

    vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-report", None)
        .expect("drop succeeds");

    let done = vp("commitments/_done/2026/quarterly-report.md");
    let fm = read_commitment_frontmatter(&store, &done);
    assert_eq!(fm.status, CommitmentStatus::Dropped);
    assert_eq!(fm.completed, None, "a drop is not dated");
    assert!(
        !store
            .exists(&vp("commitments/quarterly-report.md"))
            .unwrap(),
        "the active note is moved, not copied"
    );

    let raw = store.read_file(&done).unwrap();
    assert!(
        raw.contains("Promised to Bob in the April review."),
        "the body survives, unlike delete-and-recreate:\n{raw}"
    );

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-20.md"))
        .unwrap();
    assert!(
        daily.contains(
            "- **16:30**: commitment dropped on [[quarterly-report]] \u{2014} Quarterly report"
        ),
        "the log records a drop:\n{daily}"
    );
    assert!(
        !daily.contains("commitment completed"),
        "and never a completion:\n{daily}"
    );
}

/// The user-visible promise: a dropped commitment is not finished work.
#[test]
fn a_dropped_commitment_never_appears_in_the_commitments_view() {
    let (vault, _store) =
        vault_with_seeded_store(&[("commitments/quarterly-report.md", ACTIVE_COMMITMENT)]);

    vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-report", None)
        .expect("drop succeeds");

    let items = vault
        .commitments(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(), 60)
        .expect("commitments");
    assert!(
        !items.iter().any(|c| c.title.contains("Quarterly report")),
        "a promise that was cancelled is not a promise outstanding: {items:?}"
    );
}

/// A note hand-edited to carry a completion date must not be archived
/// as `dropped` *and* dated — a file contradicting itself.
#[test]
fn drop_commitment_clears_a_pre_existing_completed_date() {
    let note = ACTIVE_COMMITMENT.replace("completed: null", "completed: 2026-05-02");
    let (vault, store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", &note)]);

    vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-report", None)
        .expect("drop succeeds");

    let fm = read_commitment_frontmatter(&store, &vp("commitments/_done/2026/quarterly-report.md"));
    assert_eq!(fm.status, CommitmentStatus::Dropped);
    assert_eq!(fm.completed, None, "the stale date is cleared");
}

/// `completed` is optional, so a note omitting it parses and lints
/// clean — an ejected template may simply leave the line out. Failing
/// the verb over an absent key would leave no way to end that
/// commitment honestly at all.
#[test]
fn drop_commitment_survives_a_note_with_no_completed_field() {
    let note = ACTIVE_COMMITMENT.replace("completed: null\n", "");
    assert!(!note.contains("completed:"), "fixture must drop the key");
    let (vault, store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", &note)]);

    vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-report", None)
        .expect("an absent key already says what the rewrite would write");

    let fm = read_commitment_frontmatter(&store, &vp("commitments/_done/2026/quarterly-report.md"));
    assert_eq!(fm.status, CommitmentStatus::Dropped);
    assert_eq!(fm.completed, None);
}

/// The absence must be confirmed by the YAML parser, not by the
/// rewriter's column-0 line scan: a quoted key is invisible to the scan
/// and visible to the parser. Swallowing on the scan alone archives the
/// self-contradictory file this clearing exists to prevent.
#[test]
fn drop_commitment_refuses_a_completion_date_the_rewriter_cannot_see() {
    let note = ACTIVE_COMMITMENT.replace("completed: null", "\"completed\": 2026-05-02");
    let (vault, store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", &note)]);

    let err = vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-report", None)
        .expect_err("a date that cannot be cleared must not be archived");
    assert!(
        matches!(err, DomainError::MissingFrontmatterField(_)),
        "got {err:?}"
    );
    assert!(
        !store
            .exists(&vp("commitments/_done/2026/quarterly-report.md"))
            .unwrap(),
        "nothing is archived on the error path"
    );
}

/// The reason distinguishes a cancellation from a supersession a month
/// later, and rides its own line so one drop stays one entry.
#[test]
fn drop_commitment_puts_its_reason_on_a_continuation_line_flattened() {
    let (vault, store) =
        vault_with_seeded_store(&[("commitments/quarterly-report.md", ACTIVE_COMMITMENT)]);

    vault
        .drop_commitment(
            dt(2026, 5, 20, 16, 30),
            "quarterly-report",
            Some("the client\n  cancelled\n\nthe engagement"),
        )
        .expect("drop succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-20.md"))
        .unwrap();
    assert!(
        daily.contains(
            "- **16:30**: commitment dropped on [[quarterly-report]] \u{2014} Quarterly report\n  reason: the client cancelled the engagement"
        ),
        "reason on its own indented line, whitespace flattened:\n{daily}"
    );
}

#[test]
fn drop_commitment_without_a_reason_logs_a_bare_entry() {
    let (vault, store) =
        vault_with_seeded_store(&[("commitments/quarterly-report.md", ACTIVE_COMMITMENT)]);

    vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-report", None)
        .expect("drop succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-20.md"))
        .unwrap();
    assert!(!daily.contains("reason:"), "no empty reason line:\n{daily}");
}

/// Terminal means terminal: a dropped commitment has no date left to
/// move and no completion left to claim.
#[test]
fn a_dropped_commitment_can_be_neither_completed_nor_rescheduled() {
    let dropped = ACTIVE_COMMITMENT.replace("status: active", "status: dropped");
    let (vault, _store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", &dropped)]);

    let complete_err = vault
        .complete_commitment(dt(2026, 5, 21, 9, 0), "quarterly-report")
        .expect_err("a cancelled promise cannot then be kept");
    assert!(
        matches!(complete_err, DomainError::CommitmentNotActive(_)),
        "got {complete_err:?}"
    );

    let reschedule_err = vault
        .reschedule_commitment(
            dt(2026, 5, 21, 9, 0),
            "quarterly-report",
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
        )
        .expect_err("a cancelled promise has no date to move");
    assert!(
        matches!(reschedule_err, DomainError::CommitmentNotActive(_)),
        "got {reschedule_err:?}"
    );
}

/// Dropping the same slug twice in one year would overwrite the first
/// archive; refused, with nothing written.
#[test]
fn drop_commitment_refuses_a_done_collision_and_writes_nothing() {
    let (vault, store) = vault_with_seeded_store(&[
        ("commitments/quarterly-report.md", ACTIVE_COMMITMENT),
        (
            "commitments/_done/2026/quarterly-report.md",
            "---\ntype: commitment\nstatus: dropped\ndue: 2026-03-01\ncreated: 2026-01-01\ncompleted: null\ncontext: work\n---\n\n# Earlier one\n",
        ),
    ]);

    let err = vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-report", None)
        .expect_err("the destination is occupied");
    assert!(
        matches!(
            err,
            DomainError::Store(cdno_core::error::StoreError::AlreadyExists(_))
        ),
        "got {err:?}"
    );
    assert!(
        store
            .exists(&vp("commitments/quarterly-report.md"))
            .unwrap(),
        "the active note is untouched on the error path"
    );
    let archived = store
        .read_file(&vp("commitments/_done/2026/quarterly-report.md"))
        .unwrap();
    assert!(archived.contains("# Earlier one"), "and so is the archive");
}

/// The guard its own doc comment asserts, which nothing was pinning:
/// dropping an already-dropped commitment must be refused, not applied
/// twice. Its sibling `complete_commitment` has the analogue; this did
/// not, and deleting the guard left the whole suite green.
#[test]
fn drop_commitment_refuses_a_commitment_that_is_already_dropped() {
    let dropped = ACTIVE_COMMITMENT.replace("status: active", "status: dropped");
    let (vault, store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", &dropped)]);

    let err = vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-report", None)
        .expect_err("a promise already ended cannot end again");
    assert!(
        matches!(err, DomainError::CommitmentNotActive(_)),
        "got {err:?}"
    );
    assert!(
        store
            .exists(&vp("commitments/quarterly-report.md"))
            .unwrap(),
        "nothing moves on the error path"
    );
}

/// Likewise for a completed one sitting at the active path after a
/// hand-edit — the status is trusted over the location.
#[test]
fn drop_commitment_refuses_a_commitment_already_completed() {
    let completed = ACTIVE_COMMITMENT
        .replace("status: active", "status: completed")
        .replace("completed: null", "completed: 2026-05-02");
    let (vault, _store) =
        vault_with_seeded_store(&[("commitments/quarterly-report.md", &completed)]);

    let err = vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-report", None)
        .expect_err("a promise kept cannot then be dropped");
    assert!(
        matches!(err, DomainError::CommitmentNotActive(_)),
        "got {err:?}"
    );
}

/// The archive year comes from when it ended, not from when it was
/// made. `complete_commitment` pins this rule explicitly; the drop
/// fixtures all created and dropped in the same year, so the two
/// sources were indistinguishable and the rule unconstrained.
#[test]
fn drop_commitment_files_under_the_year_it_ended_not_the_year_it_was_made() {
    let note = ACTIVE_COMMITMENT.replace("created: 2026-04-01", "created: 2025-04-01");
    let (vault, store) = vault_with_seeded_store(&[("commitments/quarterly-report.md", &note)]);

    vault
        .drop_commitment(dt(2026, 1, 1, 0, 5), "quarterly-report", None)
        .expect("drop succeeds");

    assert!(
        store
            .exists(&vp("commitments/_done/2026/quarterly-report.md"))
            .unwrap(),
        "filed under the ending year"
    );
    assert!(
        !store
            .exists(&vp("commitments/_done/2025/quarterly-report.md"))
            .unwrap(),
        "not under the creation year"
    );
}

/// The not-found error carries the slug hint its doc comment promises —
/// the whole point being that a mistyped slug shows you the real ones.
#[test]
fn drop_commitment_not_found_lists_the_open_commitments() {
    let (vault, _store) =
        vault_with_seeded_store(&[("commitments/quarterly-report.md", ACTIVE_COMMITMENT)]);

    let err = vault
        .drop_commitment(dt(2026, 5, 20, 16, 30), "quarterly-repot", None)
        .expect_err("unknown slug");
    let msg = format!("{err}");
    assert!(
        msg.contains("quarterly-report"),
        "the hint must name the commitments that do exist: {msg}"
    );
}
