//! Unit tests for `Vault::add_tracking_entry` and the typed
//! `TrackingFrontmatter` parse against `MemoryVaultStore` /
//! `MemoryIndex`.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{Context, TrackingFrontmatter};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

fn empty_vault() -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

fn read_tracking_fm(store: &Arc<dyn VaultStore>, path: &VaultPath) -> TrackingFrontmatter {
    let raw = store.read_file(path).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    TrackingFrontmatter::try_from(fm).unwrap()
}

fn read_body(store: &Arc<dyn VaultStore>, path: &VaultPath) -> String {
    let raw = store.read_file(path).unwrap();
    let (_fm, body) = Frontmatter::parse(&raw).unwrap();
    body.to_owned()
}

// ---------------------------------------------------------------------
// activity-specific templates
// ---------------------------------------------------------------------

#[test]
fn add_tracking_gym_uses_gym_template_with_exercise_table() {
    let (vault, store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    let path = vault
        .add_tracking_entry(dt(2026, 4, 6, 19, 0), "health", "gym", "Energy was good.")
        .unwrap();

    assert_eq!(path, vp("stewardships/health/tracking/2026-04-06-gym.md"));
    let fm = read_tracking_fm(&store, &path);
    assert_eq!(fm.stewardship, "health");
    assert_eq!(fm.activity, "gym");
    assert_eq!(fm.date, NaiveDate::from_ymd_opt(2026, 4, 6).unwrap());
    // duration_min and routine present in template as null -> None on parse.
    assert_eq!(fm.duration_min, None);
    assert_eq!(fm.routine, None);

    let body = read_body(&store, &path);
    assert!(
        body.contains("# Gym \u{2014} 6 April 2026"),
        "body:\n{body}"
    );
    assert!(body.contains("| Exercise | Sets | Reps | Weight (kg) | Notes |"));
    assert!(body.contains("Energy was good."));
}

#[test]
fn add_tracking_body_uses_body_template_with_metric_table() {
    let (vault, store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    let path = vault
        .add_tracking_entry(dt(2026, 3, 30, 9, 0), "health", "body", "")
        .unwrap();

    assert_eq!(path, vp("stewardships/health/tracking/2026-03-30-body.md"));
    let fm = read_tracking_fm(&store, &path);
    assert_eq!(fm.activity, "body");
    // body template has no duration_min / routine fields at all.
    assert_eq!(fm.duration_min, None);
    assert_eq!(fm.routine, None);

    let body = read_body(&store, &path);
    assert!(body.contains("# Body \u{2014} 30 March 2026"));
    assert!(body.contains("| Weight"));
    assert!(body.contains("| Waist"));
}

#[test]
fn add_tracking_swim_uses_swim_template_with_set_table() {
    let (vault, store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    let path = vault
        .add_tracking_entry(dt(2026, 4, 12, 7, 30), "health", "swim", "")
        .unwrap();

    let body = read_body(&store, &path);
    assert!(body.contains("# Swim \u{2014} 12 April 2026"));
    assert!(body.contains("| Distance (m)"));
    assert!(body.contains("Stroke"));
}

#[test]
fn add_tracking_unknown_activity_falls_back_to_generic_template() {
    let (vault, store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    let path = vault
        .add_tracking_entry(dt(2026, 4, 1, 8, 0), "health", "yoga", "Felt loose.")
        .unwrap();

    assert_eq!(path, vp("stewardships/health/tracking/2026-04-01-yoga.md"));
    let fm = read_tracking_fm(&store, &path);
    assert_eq!(fm.activity, "yoga");

    let body = read_body(&store, &path);
    // Generic template title-cases the activity slug for the H1.
    assert!(
        body.contains("# Yoga \u{2014} 1 April 2026"),
        "body:\n{body}"
    );
    assert!(body.contains("Felt loose."));
    // Generic has no table block.
    assert!(
        !body.contains("|"),
        "generic template should not include a table"
    );
}

// ---------------------------------------------------------------------
// error paths
// ---------------------------------------------------------------------

#[test]
fn add_tracking_errors_on_empty_activity() {
    let (vault, _store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    let err = vault
        .add_tracking_entry(dt(2026, 4, 1, 8, 0), "health", "  ", "")
        .expect_err("empty activity should error");
    assert!(matches!(err, DomainError::EmptyField { field: "activity" }));
}

#[test]
fn add_tracking_errors_when_stewardship_missing() {
    let (vault, _store) = empty_vault();
    let err = vault
        .add_tracking_entry(dt(2026, 4, 1, 8, 0), "nonexistent", "gym", "")
        .expect_err("missing stewardship should error");
    assert!(matches!(err, DomainError::Store(StoreError::NotFound(_))));
}

#[test]
fn add_tracking_errors_on_flat_stewardship() {
    let (vault, _store) = empty_vault();
    vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .unwrap();
    let err = vault
        .add_tracking_entry(dt(2026, 4, 1, 8, 0), "finances", "gym", "")
        .expect_err("flat stewardship has no tracking subdir");
    assert!(matches!(err, DomainError::TrackingOnFlatStewardship(s) if s == "finances"));
}

#[test]
fn add_tracking_errors_on_same_day_same_activity_duplicate() {
    let (vault, _store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    vault
        .add_tracking_entry(dt(2026, 4, 1, 8, 0), "health", "gym", "")
        .unwrap();
    let err = vault
        .add_tracking_entry(dt(2026, 4, 1, 18, 0), "health", "gym", "evening session")
        .expect_err("duplicate slug should error");
    assert!(matches!(
        err,
        DomainError::Store(StoreError::AlreadyExists(_))
    ));
}

#[test]
fn add_tracking_indexes_as_tracking_type() {
    // Two tracking notes; after creation, list_stewardships reflects
    // both via the index walk it does over `type: tracking`. This
    // doubles as the indexing smoke-test for the new note type.
    let (vault, _store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    vault
        .add_tracking_entry(dt(2026, 4, 1, 8, 0), "health", "gym", "")
        .unwrap();
    vault
        .add_tracking_entry(dt(2026, 4, 2, 8, 0), "health", "body", "")
        .unwrap();
    let summaries = vault
        .list_stewardships(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap())
        .unwrap();
    assert_eq!(summaries[0].tracking_count, 2);
}
