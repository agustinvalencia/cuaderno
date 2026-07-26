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
use cdno_domain::TrackingEntryDraft;
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
// activity templates: generic built-in + per-vault variants
// ---------------------------------------------------------------------

/// A vault-authored variant template with the shape a user might want (the
/// `examples/templates/tracking/gym.md` starter): a `routine:` field and an
/// exercise table. No such template ships built-in.
const CUSTOM_GYM_TEMPLATE: &str = "---\ntype: tracking\nstewardship: {{stewardship}}\nactivity: gym\ndate: {{date}}\nduration_min: null\nroutine: {{routine}}\n---\n\n# Gym \u{2014} {{date_long}}\n\n| Exercise | Sets | Reps | Weight (kg) | Notes |\n|----------|------|------|-------------|-------|\n|          |      |      |             |       |\n\n## Notes\n{{content}}\n";

#[test]
fn add_tracking_uses_a_vault_variant_template_when_present() {
    // A vault supplies its own variant at
    // `.cuaderno/templates/tracking-<activity>.md`; the create path resolves
    // it (structured table renders) and its `routine:` field is substituted.
    let (vault, store) = empty_vault();
    store
        .write_file(
            &vp(".cuaderno/templates/tracking-gym.md"),
            CUSTOM_GYM_TEMPLATE,
        )
        .unwrap();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();

    let path = vault
        .add_tracking_entry(
            dt(2026, 4, 6, 19, 0),
            TrackingEntryDraft::new("health", "gym")
                .with_routine("upper-body-a")
                .with_content("Energy was good."),
        )
        .map(|(outcome, _)| outcome.primary)
        .unwrap();

    assert_eq!(path, vp("stewardships/health/tracking/2026-04-06-gym.md"));
    let fm = read_tracking_fm(&store, &path);
    assert_eq!(fm.activity, "gym");
    let raw = store.read_file(&path).unwrap();
    assert!(raw.contains("# Gym \u{2014} 6 April 2026"), "raw:\n{raw}");
    assert!(
        raw.contains("| Exercise | Sets | Reps | Weight (kg) | Notes |"),
        "raw:\n{raw}"
    );
    assert!(raw.contains("Energy was good."));
    // The template carries a `routine:` field, so the wikilink substitutes.
    assert!(
        raw.contains("routine: \"[[stewardships/health/routines/upper-body-a]]\""),
        "raw:\n{raw}"
    );
}

#[test]
fn add_tracking_falls_back_to_generic_without_a_variant_template() {
    let (vault, store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    let path = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "yoga").with_content("Felt loose."),
        )
        .map(|(outcome, _)| outcome.primary)
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
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "  "),
        )
        .map(|(outcome, _)| outcome.primary)
        .expect_err("empty activity should error");
    assert!(matches!(err, DomainError::EmptyField { field: "activity" }));
}

#[test]
fn add_tracking_errors_when_stewardship_missing() {
    let (vault, _store) = empty_vault();
    let err = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("nonexistent", "gym"),
        )
        .map(|(outcome, _)| outcome.primary)
        .expect_err("missing stewardship should error");
    assert!(matches!(err, DomainError::Store(StoreError::NotFound(_))));
}

#[test]
fn add_tracking_missing_stewardship_error_lists_available_slugs() {
    let (vault, _store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Gym", Context::Personal)
        .unwrap();
    vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .unwrap();
    let err = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("fitness", "gym"),
        )
        .map(|(outcome, _)| outcome.primary)
        .expect_err("invented slug should error");
    let DomainError::Store(StoreError::NotFound(msg)) = err else {
        panic!("expected NotFound, got {err:?}");
    };
    // The agent-facing message must enumerate the real slugs so a
    // client that guessed `fitness` can self-correct to `gym`.
    assert!(msg.contains("available stewardships:"), "msg was: {msg}");
    assert!(msg.contains("gym (expanded)"), "msg was: {msg}");
    assert!(msg.contains("finances"), "msg was: {msg}");
}

#[test]
fn add_tracking_errors_on_flat_stewardship() {
    let (vault, _store) = empty_vault();
    vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .unwrap();
    let err = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("finances", "gym"),
        )
        .map(|(outcome, _)| outcome.primary)
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
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "gym"),
        )
        .map(|(outcome, _)| outcome.primary)
        .unwrap();
    let err = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 18, 0),
            TrackingEntryDraft::new("health", "gym").with_content("evening session"),
        )
        .map(|(outcome, _)| outcome.primary)
        .expect_err("duplicate slug should error");
    assert!(matches!(
        err,
        DomainError::Store(StoreError::AlreadyExists(_))
    ));
}

#[test]
fn add_tracking_errors_on_prewrapped_routine() {
    let (vault, _store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    let err = vault
        .add_tracking_entry(
            dt(2026, 4, 6, 19, 0),
            TrackingEntryDraft::new("health", "gym")
                .with_routine("[[stewardships/health/routines/foo]]"),
        )
        .map(|(outcome, _)| outcome.primary)
        .expect_err("pre-wrapped routine should error");
    assert!(matches!(err, DomainError::MalformedWikilink { .. }));
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
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "gym"),
        )
        .map(|(outcome, _)| outcome.primary)
        .unwrap();
    vault
        .add_tracking_entry(
            dt(2026, 4, 2, 8, 0),
            TrackingEntryDraft::new("health", "body"),
        )
        .map(|(outcome, _)| outcome.primary)
        .unwrap();
    let summaries = vault
        .list_stewardships(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap())
        .unwrap();
    assert_eq!(summaries[0].tracking_count, 2);
}

// ---------------------------------------------------------------------
// Structured metrics, backdating, and the audit line (#481, #482)
// ---------------------------------------------------------------------

/// The frontmatter of a filed entry as the index JSON sees it — the shape a
/// query has to work with.
fn frontmatter_json(store: &Arc<dyn VaultStore>, path: &VaultPath) -> serde_json::Value {
    let raw = store.read_file(path).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    fm.as_json()
}

fn metrics(pairs: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    match pairs {
        serde_json::Value::Object(m) => m,
        other => panic!("metrics fixture must be an object, got {other}"),
    }
}

fn health_vault() -> (Vault, Arc<dyn VaultStore>) {
    let (vault, store) = empty_vault();
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    (vault, store)
}

#[test]
fn scalar_metrics_land_in_frontmatter() {
    let (vault, store) = health_vault();
    let (outcome, _source) = vault
        .add_tracking_entry(
            dt(2026, 4, 6, 19, 0),
            TrackingEntryDraft::new("health", "body").with_metrics(metrics(
                serde_json::json!({"weight": 82.5, "resting_hr": 54}),
            )),
        )
        .unwrap();

    let fm = frontmatter_json(&store, &outcome.primary);
    assert_eq!(fm.get("weight"), Some(&serde_json::json!(82.5)));
    assert_eq!(fm.get("resting_hr"), Some(&serde_json::json!(54)));
    // The template's own keys survive the merge.
    assert_eq!(fm.get("activity"), Some(&serde_json::json!("body")));
}

#[test]
fn a_record_sequence_reaches_the_index_as_a_nested_array() {
    // The shape that makes grouping possible: one entry, several comparable
    // items, each a flat record.
    let (vault, store) = health_vault();
    let (outcome, _source) = vault
        .add_tracking_entry(
            dt(2026, 4, 6, 19, 0),
            TrackingEntryDraft::new("health", "practice").with_metrics(metrics(
                serde_json::json!({
                    "detail": [
                        {"subject": "harmony", "minutes": 25, "focus": 4},
                        {"subject": "sight-reading", "minutes": 15, "focus": 5},
                    ]
                }),
            )),
        )
        .unwrap();

    let fm = frontmatter_json(&store, &outcome.primary);
    let detail = fm.get("detail").expect("detail key").as_array().unwrap();
    assert_eq!(detail.len(), 2);
    assert_eq!(detail[0]["subject"], serde_json::json!("harmony"));
    assert_eq!(detail[1]["minutes"], serde_json::json!(15));
    // The entry still parses as a tracking note — the merge must not disturb
    // the typed fields the parse requires.
    assert_eq!(
        read_tracking_fm(&store, &outcome.primary).activity,
        "practice"
    );
}

#[test]
fn a_metric_violating_a_declared_schema_errors_naming_the_field() {
    let (store, index) = (
        Arc::new(MemoryVaultStore::new()) as Arc<dyn VaultStore>,
        Arc::new(MemoryIndex::new()) as Arc<dyn VaultIndex>,
    );
    let mut schema = cdno_core::config::SchemaExtension::default();
    schema.fields.insert(
        "weight".to_owned(),
        cdno_core::config::FieldSpec {
            ty: cdno_core::config::FieldType::Float,
            default: None,
            required: false,
            values: None,
            list: None,
            settable: None,
            log_on_change: None,
        },
    );
    let mut config = VaultConfig::default();
    config.schemas.insert("tracking".to_owned(), schema);
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config).expect("Vault::new");
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();

    match vault.add_tracking_entry(
        dt(2026, 4, 6, 19, 0),
        TrackingEntryDraft::new("health", "body")
            .with_metrics(metrics(serde_json::json!({"weight": "heavy"}))),
    ) {
        Err(DomainError::InvalidFieldValue { field, reason, .. }) => {
            assert_eq!(field, "weight");
            assert!(reason.contains("not a valid float"), "reason: {reason}");
        }
        other => panic!("expected InvalidFieldValue(weight), got {other:?}"),
    }
}

#[test]
fn an_undeclared_metric_is_written_as_given() {
    // Undeclared frontmatter is legal everywhere else in the vault; a
    // per-activity declaration that could reject one does not exist yet.
    let (vault, store) = health_vault();
    let (outcome, _source) = vault
        .add_tracking_entry(
            dt(2026, 4, 6, 19, 0),
            TrackingEntryDraft::new("health", "body")
                .with_metrics(metrics(serde_json::json!({"whatever": "free text"}))),
        )
        .unwrap();

    assert_eq!(
        frontmatter_json(&store, &outcome.primary).get("whatever"),
        Some(&serde_json::json!("free text"))
    );
}

#[test]
fn an_explicit_date_files_the_entry_on_that_day() {
    let (vault, store) = health_vault();
    let (outcome, _source) = vault
        .add_tracking_entry(
            dt(2026, 4, 20, 9, 0),
            TrackingEntryDraft::new("health", "gym")
                .on(NaiveDate::from_ymd_opt(2026, 4, 6).unwrap()),
        )
        .unwrap();

    assert_eq!(
        outcome.primary,
        vp("stewardships/health/tracking/2026-04-06-gym.md")
    );
    // The note's own `date:` follows the entry, not the clock.
    assert_eq!(
        read_tracking_fm(&store, &outcome.primary).date,
        NaiveDate::from_ymd_opt(2026, 4, 6).unwrap()
    );
}

#[test]
fn an_implausible_date_is_rejected_at_both_ends() {
    let (vault, _store) = health_vault();
    let now = dt(2026, 4, 20, 9, 0);
    for date in [
        NaiveDate::from_ymd_opt(2062, 1, 1).unwrap(), // a mistyped year, far ahead
        NaiveDate::from_ymd_opt(1926, 1, 1).unwrap(), // absurdly far back
    ] {
        match vault.add_tracking_entry(now, TrackingEntryDraft::new("health", "gym").on(date)) {
            Err(DomainError::ImplausibleDate {
                date: reported,
                earliest,
                latest,
            }) => {
                assert_eq!(reported, date);
                assert!(earliest < latest, "the window must be well-formed");
            }
            other => panic!("expected ImplausibleDate for {date}, got {other:?}"),
        }
    }
}

#[test]
fn a_date_inside_the_window_is_accepted_at_both_ends() {
    // The bound exists to catch a typo, not to block a real import or a
    // deliberately future-dated entry.
    let (vault, _store) = health_vault();
    let now = dt(2026, 4, 20, 9, 0);
    for (date, activity) in [
        (NaiveDate::from_ymd_opt(1980, 6, 1).unwrap(), "gym"),
        (NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(), "swim"),
    ] {
        vault
            .add_tracking_entry(now, TrackingEntryDraft::new("health", activity).on(date))
            .unwrap_or_else(|e| panic!("{date} must be accepted, got {e:?}"));
    }
}

#[test]
fn filing_an_entry_stages_a_daily_log_line() {
    let (vault, store) = health_vault();
    let (outcome, _source) = vault
        .add_tracking_entry(
            dt(2026, 4, 6, 19, 0),
            TrackingEntryDraft::new("health", "gym"),
        )
        .unwrap();

    let daily = VaultPath::new(cdno_core::paths::daily_note_relpath(
        NaiveDate::from_ymd_opt(2026, 4, 6).unwrap(),
    ))
    .unwrap();
    assert!(
        outcome.paths.contains(&daily),
        "the daily note must be in the touched set so the desktop journals it: {:?}",
        outcome.paths
    );
    let log = store.read_file(&daily).unwrap();
    assert!(
        log.contains("Tracked gym: [[stewardships/health/tracking/2026-04-06-gym]]"),
        "daily log:\n{log}"
    );
}

#[test]
fn a_backdated_entry_logs_into_todays_note_naming_the_day_it_describes() {
    // The audit trail is the point of the bound: a write that reshapes a past
    // trend must be findable from the day it was made, not buried in the day
    // it claims to describe.
    let (vault, store) = health_vault();
    vault
        .add_tracking_entry(
            dt(2026, 4, 20, 9, 0),
            TrackingEntryDraft::new("health", "gym")
                .on(NaiveDate::from_ymd_opt(2026, 4, 6).unwrap()),
        )
        .unwrap();

    let today = VaultPath::new(cdno_core::paths::daily_note_relpath(
        NaiveDate::from_ymd_opt(2026, 4, 20).unwrap(),
    ))
    .unwrap();
    let log = store.read_file(&today).unwrap();
    assert!(
        log.contains("Tracked gym for 2026-04-06:"),
        "today's log must name the backdated day:\n{log}"
    );
    let backdated_daily = VaultPath::new(cdno_core::paths::daily_note_relpath(
        NaiveDate::from_ymd_opt(2026, 4, 6).unwrap(),
    ))
    .unwrap();
    assert!(
        !store.exists(&backdated_daily).unwrap(),
        "backfilling must not scaffold a daily note for a day that never had one"
    );
}
