//! Unit tests for `Vault::create_stewardship_flat` and
//! `Vault::create_stewardship_expanded` against `MemoryVaultStore` /
//! `MemoryIndex`.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{Context, StewardshipFrontmatter};
use cdno_domain::recurrence::Recurrence;
use cdno_domain::{StewardshipSummary, StewardshipVariant, Vault};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
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

fn read_stewardship(store: &Arc<dyn VaultStore>, path: &VaultPath) -> StewardshipFrontmatter {
    let raw = store.read_file(path).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    StewardshipFrontmatter::try_from(fm).unwrap()
}

fn read_body(store: &Arc<dyn VaultStore>, path: &VaultPath) -> String {
    let raw = store.read_file(path).unwrap();
    let (_fm, body) = Frontmatter::parse(&raw).unwrap();
    body.to_owned()
}

// ---------------------------------------------------------------------
// create_stewardship_flat
// ---------------------------------------------------------------------

#[test]
fn create_flat_writes_single_file_at_root() {
    let (vault, store) = vault_with_seeded_store(&[]);
    let path = vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .expect("create_stewardship_flat");

    assert_eq!(path, vp("stewardships/finances.md"));
    let fm = read_stewardship(&store, &path);
    assert_eq!(fm.context, Context::Household);
    let body = read_body(&store, &path);
    assert!(body.contains("# Finances"), "body:\n{body}");
    assert!(body.contains("## Current Status"));
    assert!(body.contains("## Periodic Commitments"));
}

#[test]
fn create_flat_errors_on_empty_name() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    let err = vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "   ", Context::Personal)
        .expect_err("empty name should error");
    assert!(matches!(err, DomainError::EmptyField { field: "name" }));
}

#[test]
fn create_flat_errors_when_same_flat_already_exists() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .unwrap();
    let err = vault
        .create_stewardship_flat(dt(2026, 1, 11, 9, 0), "Finances", Context::Household)
        .expect_err("duplicate flat should error");
    assert!(matches!(
        err,
        DomainError::Store(StoreError::AlreadyExists(_))
    ));
}

#[test]
fn create_flat_errors_when_expanded_with_same_slug_exists() {
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/health/_index.md",
        "---\ntype: stewardship\ncontext: personal\n---\n\n# Health\n",
    )]);
    let err = vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .expect_err("flat must not stomp existing expanded");
    let msg = format!("{err}");
    assert!(matches!(
        err,
        DomainError::Store(StoreError::AlreadyExists(_))
    ));
    assert!(msg.contains("stewardships/health/_index.md"), "msg: {msg}");
}

// ---------------------------------------------------------------------
// create_stewardship_expanded
// ---------------------------------------------------------------------

#[test]
fn create_expanded_writes_index_inside_folder() {
    let (vault, store) = vault_with_seeded_store(&[]);
    let path = vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .expect("create_stewardship_expanded");

    assert_eq!(path, vp("stewardships/health/_index.md"));
    let fm = read_stewardship(&store, &path);
    assert_eq!(fm.context, Context::Personal);
    let body = read_body(&store, &path);
    assert!(body.contains("# Health"));
    assert!(body.contains("## Active Habits"));
}

#[test]
fn create_expanded_does_not_pre_materialise_tracking_or_routines() {
    let (vault, store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    // Subdirs appear when the first file is written; with only
    // _index.md committed, no other paths under stewardships/health/
    // should exist yet.
    assert!(
        !store
            .exists(&vp("stewardships/health/tracking/.gitkeep"))
            .unwrap()
    );
    assert!(
        !store
            .exists(&vp("stewardships/health/routines/.gitkeep"))
            .unwrap()
    );
    // The index file is the only thing in the folder.
    assert!(store.exists(&vp("stewardships/health/_index.md")).unwrap());
}

#[test]
fn create_expanded_errors_on_empty_name() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    let err = vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "", Context::Personal)
        .expect_err("empty name should error");
    assert!(matches!(err, DomainError::EmptyField { field: "name" }));
}

#[test]
fn create_expanded_errors_when_same_expanded_already_exists() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    let err = vault
        .create_stewardship_expanded(dt(2026, 1, 11, 9, 0), "Health", Context::Personal)
        .expect_err("duplicate expanded should error");
    assert!(matches!(
        err,
        DomainError::Store(StoreError::AlreadyExists(_))
    ));
}

#[test]
fn create_expanded_errors_when_flat_with_same_slug_exists() {
    let (vault, _store) = vault_with_seeded_store(&[(
        "stewardships/finances.md",
        "---\ntype: stewardship\ncontext: household\n---\n\n# Finances\n",
    )]);
    let err = vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .expect_err("expanded must not stomp existing flat");
    let msg = format!("{err}");
    assert!(matches!(
        err,
        DomainError::Store(StoreError::AlreadyExists(_))
    ));
    assert!(msg.contains("stewardships/finances.md"), "msg: {msg}");
}

// ---------------------------------------------------------------------
// add_periodic_commitment
// ---------------------------------------------------------------------

#[test]
fn add_periodic_commitment_appends_line_to_flat_dashboard() {
    let (vault, store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .unwrap();
    let path = vault
        .add_periodic_commitment(
            dt(2026, 1, 10, 9, 0),
            "finances",
            "Tax declaration",
            Recurrence::Yearly,
            NaiveDate::from_ymd_opt(2026, 5, 2).unwrap(),
        )
        .expect("add_periodic_commitment");

    assert_eq!(path, vp("stewardships/finances.md"));
    let body = read_body(&store, &path);
    assert!(
        body.contains("- Tax declaration \u{2014} yearly \u{2014} next: 2026-05-02"),
        "body:\n{body}"
    );
}

#[test]
fn add_periodic_commitment_appends_line_to_expanded_dashboard() {
    let (vault, store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    let path = vault
        .add_periodic_commitment(
            dt(2026, 1, 10, 9, 0),
            "health",
            "Dental check-up",
            Recurrence::EveryNMonths(6),
            NaiveDate::from_ymd_opt(2026, 4, 15).unwrap(),
        )
        .unwrap();

    assert_eq!(path, vp("stewardships/health/_index.md"));
    let body = read_body(&store, &path);
    assert!(
        body.contains("- Dental check-up \u{2014} every 6 months \u{2014} next: 2026-04-15"),
        "body:\n{body}"
    );
}

#[test]
fn add_periodic_commitment_errors_on_empty_title() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .unwrap();
    let err = vault
        .add_periodic_commitment(
            dt(2026, 1, 10, 9, 0),
            "finances",
            "   ",
            Recurrence::Yearly,
            NaiveDate::from_ymd_opt(2026, 5, 2).unwrap(),
        )
        .expect_err("empty title should error");
    assert!(matches!(err, DomainError::EmptyField { field: "title" }));
}

#[test]
fn add_periodic_commitment_errors_when_stewardship_missing() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    let err = vault
        .add_periodic_commitment(
            dt(2026, 1, 10, 9, 0),
            "nonexistent",
            "Anything",
            Recurrence::Yearly,
            NaiveDate::from_ymd_opt(2026, 5, 2).unwrap(),
        )
        .expect_err("missing stewardship should error");
    assert!(matches!(err, DomainError::Store(_)));
}

#[test]
fn add_periodic_commitment_stacks_multiple_lines_in_order() {
    let (vault, store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .unwrap();
    vault
        .add_periodic_commitment(
            dt(2026, 1, 10, 9, 0),
            "finances",
            "Tax declaration",
            Recurrence::Yearly,
            NaiveDate::from_ymd_opt(2026, 5, 2).unwrap(),
        )
        .unwrap();
    vault
        .add_periodic_commitment(
            dt(2026, 1, 10, 9, 0),
            "finances",
            "Budget review",
            Recurrence::Monthly,
            NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
        )
        .unwrap();

    let body = read_body(&store, &vp("stewardships/finances.md"));
    let tax_idx = body.find("Tax declaration").expect("first line present");
    let budget_idx = body.find("Budget review").expect("second line present");
    assert!(tax_idx < budget_idx, "insertion order preserved");
}

// ---------------------------------------------------------------------
// list_stewardships
// ---------------------------------------------------------------------

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()
}

#[test]
fn list_stewardships_returns_empty_for_empty_vault() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    assert!(vault.list_stewardships(today()).unwrap().is_empty());
}

#[test]
fn list_stewardships_carries_name_context_variant_sorted_by_slug() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .unwrap();

    let summaries = vault.list_stewardships(today()).unwrap();
    let slugs: Vec<&str> = summaries.iter().map(|s| s.slug.as_str()).collect();
    assert_eq!(slugs, vec!["finances", "health"]);

    let fin = summaries.iter().find(|s| s.slug == "finances").unwrap();
    assert_eq!(fin.name, "Finances");
    assert_eq!(fin.context, Context::Household);
    assert_eq!(fin.variant, StewardshipVariant::Flat);
    assert_eq!(fin.tracking_count, 0);
    assert_eq!(fin.last_tracking_date, None);
    assert_eq!(fin.staleness_days, None);

    let h = summaries.iter().find(|s| s.slug == "health").unwrap();
    assert_eq!(h.variant, StewardshipVariant::Expanded);
    assert_eq!(h.tracking_count, 0);
}

#[test]
fn list_stewardships_counts_tracking_and_reports_latest_date() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();
    vault
        .add_tracking_entry(dt(2026, 4, 10, 9, 0), "health", "gym", None, "")
        .unwrap();
    vault
        .add_tracking_entry(dt(2026, 4, 20, 9, 0), "health", "body", None, "")
        .unwrap();
    vault
        .add_tracking_entry(dt(2026, 4, 15, 9, 0), "health", "swim", None, "")
        .unwrap();

    let summaries = vault.list_stewardships(today()).unwrap();
    let h = summaries.iter().find(|s| s.slug == "health").unwrap();
    assert_eq!(h.tracking_count, 3);
    assert_eq!(
        h.last_tracking_date,
        Some(NaiveDate::from_ymd_opt(2026, 4, 20).unwrap())
    );
    assert_eq!(h.staleness_days, Some(11));
}

#[test]
fn list_stewardships_keeps_flat_count_at_zero_even_if_orphan_tracking_exists() {
    // Hand-seed a tracking note that erroneously names a flat
    // stewardship. The summary must still report zero — flat
    // dashboards have no tracking subdir by design.
    let orphan = "---\ntype: tracking\nstewardship: finances\nactivity: gym\ndate: 2026-04-10\nduration_min: null\nroutine: null\n---\n\n# Gym\n";
    let (vault, _store) = vault_with_seeded_store(&[
        (
            "stewardships/finances.md",
            "---\ntype: stewardship\ncontext: household\n---\n\n# Finances\n",
        ),
        ("stewardships/finances/tracking/2026-04-10-gym.md", orphan),
    ]);

    let summaries: Vec<StewardshipSummary> = vault.list_stewardships(today()).unwrap();
    let fin = summaries.iter().find(|s| s.slug == "finances").unwrap();
    assert_eq!(fin.variant, StewardshipVariant::Flat);
    assert_eq!(fin.tracking_count, 0);
}

// ---------------------------------------------------------------------
// both variants coexist when slugs differ
// ---------------------------------------------------------------------

#[test]
fn flat_and_expanded_with_different_slugs_coexist() {
    let (vault, store) = vault_with_seeded_store(&[]);
    let flat = vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Finances", Context::Household)
        .unwrap();
    let expanded = vault
        .create_stewardship_expanded(dt(2026, 1, 10, 9, 0), "Health", Context::Personal)
        .unwrap();

    assert_eq!(flat, vp("stewardships/finances.md"));
    assert_eq!(expanded, vp("stewardships/health/_index.md"));
    assert!(store.exists(&flat).unwrap());
    assert!(store.exists(&expanded).unwrap());
    // Each carries its own context.
    assert_eq!(read_stewardship(&store, &flat).context, Context::Household);
    assert_eq!(
        read_stewardship(&store, &expanded).context,
        Context::Personal
    );
}

// ---------------------------------------------------------------------
// not-found hint (self-correcting slug errors)
// ---------------------------------------------------------------------

#[test]
fn not_found_error_lists_the_available_stewardships() {
    // The motivating failure: a caller (often an agent) guesses a slug
    // that doesn't exist. The not-found error names the valid set so it
    // can self-correct instead of guessing again.
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_stewardship_flat(dt(2026, 1, 10, 9, 0), "Gym", Context::Personal)
        .unwrap();
    vault
        .create_stewardship_expanded(dt(2026, 1, 11, 9, 0), "Health", Context::Personal)
        .unwrap();

    let err = vault.get_stewardship("fitness").unwrap_err();
    let DomainError::Store(StoreError::NotFound(msg)) = err else {
        panic!("expected Store(NotFound), got {err:?}");
    };
    assert!(
        msg.contains("available stewardships:"),
        "missing hint: {msg}"
    );
    assert!(msg.contains("gym"), "missing flat slug: {msg}");
    // Expanded stewardships are flagged — only they accept tracking notes.
    assert!(
        msg.contains("health (expanded)"),
        "missing expanded flag: {msg}"
    );
}

#[test]
fn not_found_error_has_no_hint_when_no_stewardships_exist() {
    let (vault, _store) = vault_with_seeded_store(&[]);

    let err = vault.get_stewardship("anything").unwrap_err();
    let DomainError::Store(StoreError::NotFound(msg)) = err else {
        panic!("expected Store(NotFound), got {err:?}");
    };
    // Base message still names the looked-up slug; no dangling "available"
    // suffix when there's nothing to suggest.
    assert!(msg.contains("stewardships/anything"), "msg: {msg}");
    assert!(!msg.contains("available stewardships"), "msg: {msg}");
}
