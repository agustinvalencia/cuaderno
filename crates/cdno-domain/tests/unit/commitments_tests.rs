//! Unit tests for `Vault::create_commitment` and
//! `Vault::complete_commitment`. Uses `MemoryVaultStore` /
//! `MemoryIndex` so the suite stays fast and deterministic — no
//! disk I/O.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{CommitmentFrontmatter, CommitmentStatus, Context};
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
fn create_commitment_logs_creation_to_daily_note() {
    let (vault, store) = vault_with_seeded_store(&[]);

    vault
        .create_commitment(
            dt(2026, 5, 2, 9, 0),
            "Renew passport",
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            Context::Personal,
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
