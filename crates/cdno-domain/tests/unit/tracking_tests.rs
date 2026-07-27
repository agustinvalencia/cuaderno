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
fn a_second_entry_the_same_day_merges_rather_than_erroring() {
    // Several domains are naturally multi-occurrence - spending happens
    // through the day, contact more than once, practice splits morning and
    // evening - and for agent-driven logging a day recorded in two passes is
    // ordinary rather than exceptional.
    let (vault, store) = health_vault();
    let (first, _source) = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "gym").with_content("Morning session."),
        )
        .unwrap();
    let (second, source) = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 18, 0),
            TrackingEntryDraft::new("health", "gym").with_content("Evening session."),
        )
        .expect("a second entry the same day merges");

    assert_eq!(first.primary, second.primary, "one note, not two");
    assert_eq!(source, None, "a merge resolves no template");
    let raw = store.read_file(&second.primary).unwrap();
    assert!(raw.contains("Morning session."), "{raw}");
    assert!(
        raw.contains("Evening session."),
        "tracking notes are append-only: {raw}"
    );
}

#[test]
fn re_applying_records_with_matching_ids_leaves_a_sum_unchanged() {
    // Removing the duplicate guard without an identity would make re-running
    // an import append the same records again and double-count every `sum`.
    // Reconciled domains are exactly where re-runs happen.
    let (vault, store) = health_vault();
    let payload = || {
        metrics(serde_json::json!({
            "detail": [
                {"id": "a", "category": "groceries", "amount": 40},
                {"id": "b", "category": "transport", "amount": 12},
            ]
        }))
    };
    let (first, _) = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "spending").with_metrics(payload()),
        )
        .unwrap();
    vault
        .add_tracking_entry(
            dt(2026, 4, 1, 20, 0),
            TrackingEntryDraft::new("health", "spending").with_metrics(payload()),
        )
        .expect("re-applying merges");

    let fm = frontmatter_json(&store, &first.primary);
    let detail = fm["detail"].as_array().unwrap();
    assert_eq!(detail.len(), 2, "identical ids replace, not append: {fm}");
    let total: f64 = detail.iter().filter_map(|r| r["amount"].as_f64()).sum();
    assert_eq!(total, 52.0, "a re-run must not double-count");
}

#[test]
fn a_record_without_an_id_appends() {
    // Documented, not discovered: without an id there is nothing to key on,
    // so a re-run double-counts. Import paths should supply one.
    let (vault, store) = health_vault();
    let (first, _) = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "spending")
                .with_metrics(metrics(serde_json::json!({"detail": [{"amount": 40}]}))),
        )
        .unwrap();
    vault
        .add_tracking_entry(
            dt(2026, 4, 1, 20, 0),
            TrackingEntryDraft::new("health", "spending")
                .with_metrics(metrics(serde_json::json!({"detail": [{"amount": 12}]}))),
        )
        .unwrap();

    let fm = frontmatter_json(&store, &first.primary);
    let detail = fm["detail"].as_array().unwrap();
    assert_eq!(detail.len(), 2);
    assert_eq!(detail[0]["amount"], serde_json::json!(40), "in order: {fm}");
    assert_eq!(detail[1]["amount"], serde_json::json!(12));
}

#[test]
fn a_scalar_metric_is_last_write_wins_on_merge() {
    // No array to key on, so the later reading replaces the earlier - which
    // is what a level means.
    let (vault, store) = health_vault();
    let (first, _) = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "body")
                .with_metrics(metrics(serde_json::json!({"weight": 82.5}))),
        )
        .unwrap();
    vault
        .add_tracking_entry(
            dt(2026, 4, 1, 20, 0),
            TrackingEntryDraft::new("health", "body")
                .with_metrics(metrics(serde_json::json!({"weight": 82.1}))),
        )
        .unwrap();

    assert_eq!(
        frontmatter_json(&store, &first.primary)["weight"],
        serde_json::json!(82.1)
    );
}

#[test]
fn a_merged_day_reduces_to_one_point_per_series() {
    // Merge does not add a level: the two passes share a (series, date) cell
    // and the metric's own aggregate reduces across both.
    use cdno_core::config::{Aggregate, MetricSpec, TrackingSpec};
    let (vault, _store) = health_vault();
    for amount in [40, 12] {
        vault
            .add_tracking_entry(
                dt(2026, 4, 1, 8, 0),
                TrackingEntryDraft::new("health", "spending")
                    .with_metrics(metrics(serde_json::json!({"detail": [{"amount": amount}]}))),
            )
            .unwrap();
    }
    let spec = TrackingSpec {
        records: Some("detail".to_owned()),
        group_by: None,
        metrics: [(
            "amount".to_owned(),
            MetricSpec {
                aggregate: Aggregate::Sum,
                ..Default::default()
            },
        )]
        .into_iter()
        .collect(),
    };
    let specs: std::collections::BTreeMap<String, TrackingSpec> =
        [("spending".to_owned(), spec)].into_iter().collect();

    let series = vault
        .tracking_series_from_frontmatter("health", &specs)
        .unwrap();
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].points.len(), 1, "one point for the merged day");
    assert_eq!(series[0].points[0].value, 52.0);
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

#[test]
fn a_metric_naming_an_identity_key_is_refused() {
    // The severe case: these four keys identify the note, and every reader
    // that scans tracking notes parses before it filters — so one entry whose
    // `activity` is an integer fails the read for every stewardship in the
    // vault, not just this one.
    let (vault, _store) = health_vault();
    for key in ["type", "stewardship", "activity", "date"] {
        match vault.add_tracking_entry(
            dt(2026, 4, 6, 19, 0),
            TrackingEntryDraft::new("health", "gym")
                .with_metrics(metrics(serde_json::json!({key: "hijacked"}))),
        ) {
            Err(DomainError::ReservedSchemaField { field, .. }) => assert_eq!(field, key),
            other => panic!("expected ReservedSchemaField({key}), got {other:?}"),
        }
    }
}

#[test]
fn a_refused_identity_metric_writes_nothing() {
    let (vault, store) = health_vault();
    let _ = vault.add_tracking_entry(
        dt(2026, 4, 6, 19, 0),
        TrackingEntryDraft::new("health", "gym")
            .with_metrics(metrics(serde_json::json!({"activity": 5}))),
    );
    assert!(
        !store
            .exists(&vp("stewardships/health/tracking/2026-04-06-gym.md"))
            .unwrap(),
        "a rejected payload must leave no note behind"
    );
}

#[test]
fn an_optional_typed_field_is_still_writable_as_a_metric() {
    // `duration_min` and `routine` are ordinary optional fields, not identity
    // — and a duration is a perfectly good metric.
    let (vault, store) = health_vault();
    let (outcome, _source) = vault
        .add_tracking_entry(
            dt(2026, 4, 6, 19, 0),
            TrackingEntryDraft::new("health", "gym")
                .with_metrics(metrics(serde_json::json!({"duration_min": 45}))),
        )
        .unwrap();

    assert_eq!(
        read_tracking_fm(&store, &outcome.primary).duration_min,
        Some(45)
    );
}

#[test]
fn an_extra_required_only_schema_does_not_type_check_metrics() {
    // `extra_required` desugars to an untyped *string* spec and is documented
    // and implemented as lint-only (lint gates its value-check on a non-empty
    // `fields` block). Without the same gate here, a vault that merely lists
    // `weight` as required could not write `weight: 82.5` at all.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "tracking".to_owned(),
        cdno_core::config::SchemaExtension {
            extra_required: vec!["weight".to_owned()],
            ..Default::default()
        },
    );
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config).expect("Vault::new");
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();

    vault
        .add_tracking_entry(
            dt(2026, 4, 6, 19, 0),
            TrackingEntryDraft::new("health", "body")
                .with_metrics(metrics(serde_json::json!({"weight": 82.5}))),
        )
        .expect("a lint-only extra_required must not block a numeric metric");
}

#[test]
fn a_merge_cannot_commit_a_note_the_fresh_path_would_refuse() {
    // The asymmetry that matters: both paths must enforce the same invariant.
    // A payload refused outright as a first entry must not slip in simply
    // because it arrived second - one note that no longer parses fails
    // `list_tracking`/`list_stewardships` for every stewardship in the vault,
    // since those readers parse before they filter.
    let (vault, store) = health_vault();
    let bad = || metrics(serde_json::json!({"duration_min": -5}));

    // Refused as a first entry, leaving nothing behind.
    let first = vault.add_tracking_entry(
        dt(2026, 4, 1, 8, 0),
        TrackingEntryDraft::new("health", "gym").with_metrics(bad()),
    );
    assert!(first.is_err(), "a negative duration is not a u32");
    assert!(
        !store
            .exists(&vp("stewardships/health/tracking/2026-04-01-gym.md"))
            .unwrap()
    );

    // And refused as a merge, leaving the existing entry intact.
    vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "gym").with_content("Morning."),
        )
        .unwrap();
    let before = store
        .read_file(&vp("stewardships/health/tracking/2026-04-01-gym.md"))
        .unwrap();
    assert!(
        vault
            .add_tracking_entry(
                dt(2026, 4, 1, 20, 0),
                TrackingEntryDraft::new("health", "gym").with_metrics(bad()),
            )
            .is_err(),
        "the merge path must enforce the same invariant"
    );
    assert_eq!(
        store
            .read_file(&vp("stewardships/health/tracking/2026-04-01-gym.md"))
            .unwrap(),
        before,
        "a refused merge must leave the note untouched"
    );
}

#[test]
fn a_merge_refuses_to_replace_a_record_set_with_a_scalar() {
    // Tracking notes are append-only, and the tool description shows both a
    // scalar and an array shape for `metrics` - so a caller sending the wrong
    // one is a plausible slip rather than an intent to discard the day.
    let (vault, store) = health_vault();
    let (first, _) = vault
        .add_tracking_entry(
            dt(2026, 4, 1, 8, 0),
            TrackingEntryDraft::new("health", "spending").with_metrics(metrics(
                serde_json::json!({"detail": [{"id": "a", "amount": 40}]}),
            )),
        )
        .unwrap();

    match vault.add_tracking_entry(
        dt(2026, 4, 1, 20, 0),
        TrackingEntryDraft::new("health", "spending")
            .with_metrics(metrics(serde_json::json!({"detail": 5}))),
    ) {
        Err(DomainError::InvalidFieldValue { field, reason, .. }) => {
            assert_eq!(field, "detail");
            assert!(reason.contains("record"), "reason: {reason}");
        }
        other => panic!("expected InvalidFieldValue(detail), got {other:?}"),
    }

    let detail = frontmatter_json(&store, &first.primary);
    assert_eq!(
        detail["detail"].as_array().unwrap().len(),
        1,
        "the day's records survive"
    );
}

#[test]
fn a_merge_still_records_content_when_the_notes_heading_is_ambiguous() {
    // A note carrying two `## Notes` headings cannot resolve a unique target.
    // Losing the whole merge - metrics included - over an ambiguity in the
    // prose section is the wrong trade; the content still lands.
    let (vault, store) = health_vault();
    let path = vp("stewardships/health/tracking/2026-04-01-gym.md");
    store
        .write_file(
            &path,
            "---\ntype: tracking\nstewardship: health\nactivity: gym\ndate: 2026-04-01\n---\n\n# Gym\n\n## Notes\nfirst\n\n## Notes\nsecond\n",
        )
        .unwrap();

    vault
        .add_tracking_entry(
            dt(2026, 4, 1, 20, 0),
            TrackingEntryDraft::new("health", "gym")
                .with_content("Evening.")
                .with_metrics(metrics(serde_json::json!({"duration_min": 45}))),
        )
        .expect("an ambiguous heading must not refuse the write");

    let raw = store.read_file(&path).unwrap();
    assert!(raw.contains("Evening."), "content is on the record: {raw}");
    assert!(raw.contains("duration_min: 45"), "metrics landed: {raw}");
}
