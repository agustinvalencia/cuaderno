//! Tests for `Vault::set_frontmatter` — the generic, schema-driven
//! frontmatter setter (#301).
//!
//! The load-bearing property is *no index desync*: a set must update the file
//! and the SQLite index row in one commit, so these tests assert on both the
//! re-read file and `index.find_by_path`.

use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::config::{FieldSpec, FieldType, SchemaExtension, VaultConfig};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 9).unwrap()
}

fn moment() -> NaiveDateTime {
    date().and_hms_opt(9, 30, 0).unwrap()
}

fn daily_path() -> VaultPath {
    VaultPath::new(cdno_core::paths::daily_note_relpath(date())).unwrap()
}

/// A scalar field spec with the given `settable`/`log_on_change`/`values`.
fn field(
    ty: FieldType,
    settable: Option<bool>,
    log_on_change: Option<bool>,
    values: Option<Vec<String>>,
) -> FieldSpec {
    FieldSpec {
        ty,
        default: None,
        required: false,
        values,
        list: None,
        settable,
        log_on_change,
    }
}

/// A config declaring a spread of daily fields exercising every branch:
/// settable/non-settable, logging/silent, an enum, and a would-be `status`.
fn daily_config() -> VaultConfig {
    let mut schema = SchemaExtension::default();
    schema.fields.insert(
        "meds".to_owned(),
        field(FieldType::Bool, Some(true), None, None),
    );
    schema.fields.insert(
        "workout".to_owned(),
        field(FieldType::Bool, Some(true), Some(true), None),
    );
    // Explicit `settable = false` and the default (None) both deny.
    schema.fields.insert(
        "locked".to_owned(),
        field(FieldType::Bool, Some(false), None, None),
    );
    schema.fields.insert(
        "readonly".to_owned(),
        field(FieldType::Bool, None, None, None),
    );
    schema.fields.insert(
        "mood".to_owned(),
        field(
            FieldType::String,
            Some(true),
            None,
            Some(vec!["low".to_owned(), "ok".to_owned(), "good".to_owned()]),
        ),
    );
    // `status` is *not* hard-reserved at config load, so a vault can declare it
    // settable — but `set_frontmatter` must still block it (lifecycle-owned).
    schema.fields.insert(
        "status".to_owned(),
        field(FieldType::Bool, Some(true), None, None),
    );
    let mut config = VaultConfig::default();
    config.schemas.insert("daily".to_owned(), schema);
    config
}

/// The seeded daily note carrying the toggle fields and a `## Logs` section.
const DAILY_NOTE: &str = "---\n\
type: daily\n\
date: 2026-07-09\n\
meds: false\n\
workout: false\n\
mood: ok\n\
---\n\
\n\
# Thursday\n\
\n\
## Logs\n";

/// Build a vault with the daily note seeded and the daily schema loaded,
/// returning the vault plus handles to the store and index so tests can prove
/// the file and index stay in sync.
fn seeded_vault() -> (Vault, Arc<dyn VaultStore>, Arc<dyn VaultIndex>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    store.write_file(&daily_path(), DAILY_NOTE).unwrap();
    let (vault, _report) =
        Vault::new(Arc::clone(&store), Arc::clone(&index), daily_config()).expect("Vault::new");
    (vault, store, index)
}

/// The frontmatter JSON the index holds for `path`.
fn index_frontmatter(index: &Arc<dyn VaultIndex>, path: &VaultPath) -> serde_json::Value {
    index
        .find_by_path(path)
        .unwrap()
        .expect("indexed note")
        .frontmatter
}

#[test]
fn settable_bool_toggles_and_keeps_index_in_sync() {
    let (vault, store, index) = seeded_vault();
    let outcome = vault
        .set_frontmatter(moment(), "today", "meds", "true")
        .expect("set succeeds");

    assert!(outcome.touched(), "a real change must report touched");
    assert_eq!(outcome.primary, daily_path());

    // File updated.
    let raw = store.read_file(&daily_path()).unwrap();
    assert!(raw.contains("meds: true"), "file frontmatter: {raw}");

    // Index row updated in the *same* commit — no desync.
    let fm = index_frontmatter(&index, &daily_path());
    assert_eq!(
        fm.get("meds"),
        Some(&serde_json::Value::Bool(true)),
        "index frontmatter must reflect the new value: {fm}"
    );
}

#[test]
fn a_yyyy_mm_dd_note_reference_resolves_to_the_daily_note() {
    let (vault, store, _index) = seeded_vault();
    vault
        .set_frontmatter(moment(), "2026-07-09", "meds", "true")
        .expect("date reference resolves to the daily note");
    let raw = store.read_file(&daily_path()).unwrap();
    assert!(raw.contains("meds: true"), "{raw}");
}

#[test]
fn non_settable_field_is_rejected_for_both_false_and_none() {
    let (vault, _store, _index) = seeded_vault();
    // Explicit `settable = false`.
    match vault.set_frontmatter(moment(), "today", "locked", "true") {
        Err(DomainError::FieldNotSettable { field, .. }) => assert_eq!(field, "locked"),
        other => panic!("expected FieldNotSettable(locked), got {other:?}"),
    }
    // Default-deny: `settable` unset (None).
    match vault.set_frontmatter(moment(), "today", "readonly", "true") {
        Err(DomainError::FieldNotSettable { field, .. }) => assert_eq!(field, "readonly"),
        other => panic!("expected FieldNotSettable(readonly), got {other:?}"),
    }
}

#[test]
fn an_undeclared_key_is_rejected() {
    let (vault, _store, _index) = seeded_vault();
    match vault.set_frontmatter(moment(), "today", "not_declared", "true") {
        Err(DomainError::UndeclaredSchemaField { field, .. }) => assert_eq!(field, "not_declared"),
        other => panic!("expected UndeclaredSchemaField, got {other:?}"),
    }
}

#[test]
fn a_declared_settable_status_is_still_reserved() {
    let (vault, _store, _index) = seeded_vault();
    // `status` is declared `settable = true` in the config, yet the setter
    // blocks it regardless because the lifecycle tools own it.
    match vault.set_frontmatter(moment(), "today", "status", "true") {
        Err(DomainError::ReservedSchemaField { field, .. }) => assert_eq!(field, "status"),
        other => panic!("expected ReservedSchemaField(status), got {other:?}"),
    }
}

#[test]
fn the_period_key_is_blocked() {
    let (vault, _store, _index) = seeded_vault();
    // `date` is the daily period key: it can't even be declared (config load
    // rejects that), so it surfaces here as undeclared — either way, blocked.
    let err = vault
        .set_frontmatter(moment(), "today", "date", "2026-01-01")
        .unwrap_err();
    assert!(
        matches!(err, DomainError::UndeclaredSchemaField { .. }),
        "the period key must be blocked, got {err:?}"
    );
}

#[test]
fn a_bad_typed_value_is_rejected() {
    let (vault, store, _index) = seeded_vault();
    match vault.set_frontmatter(moment(), "today", "meds", "maybe") {
        Err(DomainError::InvalidFieldValue { field, reason, .. }) => {
            assert_eq!(field, "meds");
            assert!(reason.contains("bool"), "{reason}");
        }
        other => panic!("expected InvalidFieldValue, got {other:?}"),
    }
    // The file was not touched.
    assert!(
        store
            .read_file(&daily_path())
            .unwrap()
            .contains("meds: false")
    );
}

#[test]
fn a_value_outside_the_enum_is_rejected() {
    let (vault, _store, _index) = seeded_vault();
    match vault.set_frontmatter(moment(), "today", "mood", "elated") {
        Err(DomainError::InvalidFieldValue { field, .. }) => assert_eq!(field, "mood"),
        other => panic!("expected InvalidFieldValue(mood), got {other:?}"),
    }
}

#[test]
fn no_change_is_a_silent_noop() {
    let (vault, store, _index) = seeded_vault();
    let before = store.read_file(&daily_path()).unwrap();
    // `meds` is already `false`.
    let outcome = vault
        .set_frontmatter(moment(), "today", "meds", "false")
        .expect("no-op succeeds");
    assert!(!outcome.touched(), "a no-op must report nothing touched");
    assert!(outcome.paths.is_empty());
    assert_eq!(
        store.read_file(&daily_path()).unwrap(),
        before,
        "a no-op must not rewrite the file"
    );
}

#[test]
fn log_on_change_stamps_a_daily_line_on_a_real_change() {
    let (vault, store, index) = seeded_vault();
    let outcome = vault
        .set_frontmatter(moment(), "today", "workout", "true")
        .expect("set succeeds");
    assert!(outcome.touched());

    let raw = store.read_file(&daily_path()).unwrap();
    // Both the frontmatter change and the log line land in one write — the
    // field edit is NOT clobbered by the same-note log write.
    assert!(raw.contains("workout: true"), "frontmatter: {raw}");
    assert!(
        raw.contains("workout: false \u{2192} true on [["),
        "expected a `was -> now` log line: {raw}"
    );
    assert!(raw.contains("**09:30**"), "log line is timestamped: {raw}");

    // Index reflects the new frontmatter value, not the pre-change one.
    let fm = index_frontmatter(&index, &daily_path());
    assert_eq!(fm.get("workout"), Some(&serde_json::Value::Bool(true)));
}

#[test]
fn log_on_change_stamps_nothing_on_a_noop() {
    let (vault, store, _index) = seeded_vault();
    // `workout` is already `false`; a no-op must not log.
    let outcome = vault
        .set_frontmatter(moment(), "today", "workout", "false")
        .expect("no-op succeeds");
    assert!(!outcome.touched());
    let raw = store.read_file(&daily_path()).unwrap();
    assert!(
        !raw.contains("**09:30**"),
        "a no-op must not stamp a log line: {raw}"
    );
}

#[test]
fn a_required_but_absent_key_errors_strict_exists() {
    // Declare a settable field the seeded note does NOT carry a line for.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    store.write_file(&daily_path(), DAILY_NOTE).unwrap();
    let mut schema = SchemaExtension::default();
    // `closed` is declared + settable but the note has no `closed:` line.
    schema.fields.insert(
        "closed".to_owned(),
        field(FieldType::Bool, Some(true), None, None),
    );
    let mut config = VaultConfig::default();
    config.schemas.insert("daily".to_owned(), schema);
    let (vault, _report) =
        Vault::new(Arc::clone(&store), Arc::clone(&index), config).expect("Vault::new");

    let err = vault
        .set_frontmatter(moment(), "today", "closed", "true")
        .unwrap_err();
    assert!(
        matches!(err, DomainError::MissingFrontmatterField(ref f) if f == "closed"),
        "strict-exists: an absent key must error, got {err:?}"
    );
}

#[test]
fn a_missing_note_is_not_found() {
    let (vault, _store, _index) = seeded_vault();
    let err = vault
        .set_frontmatter(moment(), "projects/does-not-exist.md", "meds", "true")
        .unwrap_err();
    assert!(
        matches!(
            err,
            DomainError::Store(cdno_core::error::StoreError::NotFound(_))
        ),
        "got {err:?}"
    );
}
