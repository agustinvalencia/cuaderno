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
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{Context, StewardshipFrontmatter};
use cdno_domain::recurrence::Recurrence;
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
