//! Unit tests for `Vault::create_portfolio` and `Vault::file_evidence`
//! plus the new `PortfolioFrontmatter` / `EvidenceFrontmatter` types.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{EvidenceFrontmatter, PortfolioFrontmatter};
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

fn read_portfolio_frontmatter(
    store: &Arc<dyn VaultStore>,
    path: &VaultPath,
) -> PortfolioFrontmatter {
    let raw = store.read_file(path).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    PortfolioFrontmatter::try_from(fm).unwrap()
}

fn read_evidence_frontmatter(store: &Arc<dyn VaultStore>, path: &VaultPath) -> EvidenceFrontmatter {
    let raw = store.read_file(path).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    EvidenceFrontmatter::try_from(fm).unwrap()
}

// ---------------------------------------------------------------------
// create_portfolio
// ---------------------------------------------------------------------

#[test]
fn create_portfolio_writes_index_with_question_and_created() {
    let (vault, store) = vault_with_seeded_store(&[]);

    let path = vault
        .create_portfolio(
            dt(2026, 2, 1, 9, 0),
            "Does the sparse variant outperform dense on OOD data?",
            None,
        )
        .expect("create succeeds");

    assert_eq!(
        path,
        vp("portfolios/does-the-sparse-variant-outperform-dense/_index.md")
    );
    let fm = read_portfolio_frontmatter(&store, &path);
    assert_eq!(fm.created, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
    assert!(fm.question.starts_with("Does the sparse variant"));
    assert!(
        fm.project.is_none(),
        "no project supplied \u{2014} should be null"
    );
}

#[test]
fn create_portfolio_wraps_optional_project_in_wikilink() {
    let (vault, store) = vault_with_seeded_store(&[]);

    let path = vault
        .create_portfolio(
            dt(2026, 2, 1, 9, 0),
            "Sparse vs dense OOD",
            Some("projects/surrogate-model"),
        )
        .expect("create succeeds");

    let raw = store.read_file(&path).unwrap();
    assert!(
        raw.contains("project: \"[[projects/surrogate-model]]\""),
        "wikilink wrapped in quotes:\n{raw}"
    );
    let fm = read_portfolio_frontmatter(&store, &path);
    assert_eq!(fm.project.as_deref(), Some("[[projects/surrogate-model]]"));
}

#[test]
fn create_portfolio_rejects_empty_question() {
    let (vault, _store) = vault_with_seeded_store(&[]);

    let err = vault
        .create_portfolio(dt(2026, 2, 1, 9, 0), "   ", None)
        .unwrap_err();
    assert!(
        matches!(err, DomainError::EmptyField { field: "question" }),
        "got {err:?}"
    );
}

#[test]
fn create_portfolio_errors_on_slug_collision() {
    let existing = "---\ntype: portfolio\nquestion: \"prior\"\ncreated: 2026-01-01\nproject: null\n---\n\n# prior\n";
    let (vault, _store) = vault_with_seeded_store(&[(
        "portfolios/does-the-sparse-variant-outperform-dense/_index.md",
        existing,
    )]);

    let err = vault
        .create_portfolio(
            dt(2026, 2, 1, 9, 0),
            "Does the sparse variant outperform dense on OOD data?",
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
// file_evidence
// ---------------------------------------------------------------------

fn seeded_vault_with_one_portfolio() -> (Vault, Arc<dyn VaultStore>) {
    let portfolio = "---\ntype: portfolio\nquestion: \"Sparse vs dense\"\ncreated: 2026-02-01\nproject: null\n---\n\n# Sparse vs dense\n";
    vault_with_seeded_store(&[("portfolios/sparse-vs-dense/_index.md", portfolio)])
}

#[test]
fn file_evidence_writes_note_with_required_origin() {
    let (vault, store) = seeded_vault_with_one_portfolio();

    let path = vault
        .file_evidence(
            dt(2026, 3, 15, 10, 0),
            "sparse-vs-dense",
            "Chen et al. 2025",
            "projects/surrogate-model",
            "They show sparse attention preserves 95% accuracy at 4x speedup.\n",
        )
        .expect("file succeeds");

    assert_eq!(
        path,
        vp("portfolios/sparse-vs-dense/2026-03-15-chen-et-al-2025.md")
    );
    let raw = store.read_file(&path).unwrap();
    assert!(raw.contains("origin: \"[[projects/surrogate-model]]\""));
    let fm = read_evidence_frontmatter(&store, &path);
    assert_eq!(fm.source, "Chen et al. 2025");
    assert_eq!(fm.portfolio, "sparse-vs-dense");
    assert_eq!(fm.origin, "[[projects/surrogate-model]]");
    assert_eq!(fm.created, NaiveDate::from_ymd_opt(2026, 3, 15).unwrap());
    assert!(raw.contains("They show sparse attention preserves"));
}

#[test]
fn file_evidence_errors_when_portfolio_missing() {
    let (vault, _store) = vault_with_seeded_store(&[]);

    let err = vault
        .file_evidence(
            dt(2026, 3, 15, 10, 0),
            "ghost-portfolio",
            "Source",
            "projects/foo",
            "body",
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
fn file_evidence_rejects_empty_source() {
    let (vault, _store) = seeded_vault_with_one_portfolio();

    let err = vault
        .file_evidence(
            dt(2026, 3, 15, 10, 0),
            "sparse-vs-dense",
            "   ",
            "projects/foo",
            "body",
        )
        .unwrap_err();
    assert!(
        matches!(err, DomainError::EmptyField { field: "source" }),
        "got {err:?}"
    );
}

#[test]
fn file_evidence_rejects_empty_origin() {
    let (vault, _store) = seeded_vault_with_one_portfolio();

    let err = vault
        .file_evidence(
            dt(2026, 3, 15, 10, 0),
            "sparse-vs-dense",
            "Chen 2025",
            "",
            "body",
        )
        .unwrap_err();
    assert!(
        matches!(err, DomainError::EmptyField { field: "origin" }),
        "got {err:?}"
    );
}

#[test]
fn file_evidence_rejects_prewrapped_origin() {
    let (vault, _store) = seeded_vault_with_one_portfolio();

    let err = vault
        .file_evidence(
            dt(2026, 3, 15, 10, 0),
            "sparse-vs-dense",
            "Chen 2025",
            "[[projects/foo]]",
            "body",
        )
        .unwrap_err();
    assert!(
        matches!(err, DomainError::MalformedWikilink { .. }),
        "got {err:?}"
    );
}

#[test]
fn file_evidence_errors_on_same_day_same_source_collision() {
    let (vault, _store) = seeded_vault_with_one_portfolio();
    vault
        .file_evidence(
            dt(2026, 3, 15, 10, 0),
            "sparse-vs-dense",
            "Chen 2025",
            "projects/foo",
            "first body",
        )
        .unwrap();

    let err = vault
        .file_evidence(
            dt(2026, 3, 15, 14, 0),
            "sparse-vs-dense",
            "Chen 2025",
            "projects/foo",
            "second body",
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
// Frontmatter parsing
// ---------------------------------------------------------------------

#[test]
fn evidence_frontmatter_requires_origin_field() {
    // A hand-rolled evidence note missing `origin:` must fail to parse,
    // matching the §5.5 rule that the field is required from Phase 3.
    let raw = "---\ntype: evidence\ncreated: 2026-03-15\nsource: \"Chen\"\nportfolio: sparse-vs-dense\n---\nbody\n";
    let (fm, _) = Frontmatter::parse(raw).unwrap();
    let err = EvidenceFrontmatter::try_from(fm).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("origin"), "error message: {msg}");
}

#[test]
fn portfolio_frontmatter_treats_project_as_optional() {
    let raw =
        "---\ntype: portfolio\nquestion: \"why\"\ncreated: 2026-02-01\nproject: null\n---\n# why\n";
    let (fm, _) = Frontmatter::parse(raw).unwrap();
    let pf = PortfolioFrontmatter::try_from(fm).unwrap();
    assert!(pf.project.is_none());
}
