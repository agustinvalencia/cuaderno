//! Unit tests for `Vault::create_portfolio` and `Vault::file_evidence`
//! plus the new `PortfolioFrontmatter` / `EvidenceFrontmatter` types.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::PortfolioSummary;
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

// ---------------------------------------------------------------------
// list_portfolios + get_portfolio_contents (#37)
// ---------------------------------------------------------------------

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

#[test]
fn list_portfolios_returns_empty_on_fresh_vault() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    let out = vault.list_portfolios(ymd(2026, 4, 1)).unwrap();
    assert!(out.is_empty(), "got {out:?}");
}

#[test]
fn list_portfolios_with_no_evidence_reports_zero_and_none() {
    let (vault, _store) = seeded_vault_with_one_portfolio();

    let out = vault.list_portfolios(ymd(2026, 4, 1)).unwrap();
    assert_eq!(out.len(), 1);
    let p = &out[0];
    assert_eq!(p.slug, "sparse-vs-dense");
    assert_eq!(p.evidence_count, 0);
    assert_eq!(p.last_updated, None);
    assert_eq!(p.staleness_days, None);
}

#[test]
fn list_portfolios_counts_evidence_and_finds_most_recent() {
    let (vault, _store) = seeded_vault_with_one_portfolio();
    vault
        .file_evidence(
            dt(2026, 3, 15, 9, 0),
            "sparse-vs-dense",
            "Chen 2025",
            "projects/foo",
            "early",
        )
        .unwrap();
    vault
        .file_evidence(
            dt(2026, 3, 22, 9, 0),
            "sparse-vs-dense",
            "Ablation B",
            "projects/foo",
            "later",
        )
        .unwrap();

    let out = vault.list_portfolios(ymd(2026, 4, 1)).unwrap();
    assert_eq!(out.len(), 1);
    let p = &out[0];
    assert_eq!(p.evidence_count, 2);
    assert_eq!(p.last_updated, Some(ymd(2026, 3, 22)));
    // 1 Apr - 22 Mar = 10 days.
    assert_eq!(p.staleness_days, Some(10));
}

#[test]
fn list_portfolios_groups_by_portfolio_and_sorts_by_slug() {
    // Seed two portfolios at known slugs; file one piece of evidence
    // each, with different dates. The output must group correctly and
    // sort alphabetically.
    let portfolio_a =
        "---\ntype: portfolio\nquestion: \"A\"\ncreated: 2026-02-01\nproject: null\n---\n# A\n";
    let portfolio_b =
        "---\ntype: portfolio\nquestion: \"B\"\ncreated: 2026-02-01\nproject: null\n---\n# B\n";
    let (vault, _store) = vault_with_seeded_store(&[
        ("portfolios/zebra/_index.md", portfolio_b),
        ("portfolios/alpha/_index.md", portfolio_a),
    ]);
    vault
        .file_evidence(dt(2026, 3, 1, 9, 0), "alpha", "src", "projects/foo", "x")
        .unwrap();
    vault
        .file_evidence(dt(2026, 3, 20, 9, 0), "zebra", "src", "projects/foo", "y")
        .unwrap();

    let out = vault.list_portfolios(ymd(2026, 4, 1)).unwrap();
    let slugs: Vec<&str> = out.iter().map(|p| p.slug.as_str()).collect();
    assert_eq!(slugs, vec!["alpha", "zebra"], "sorted by slug");
    assert_eq!(out[0].evidence_count, 1);
    assert_eq!(out[0].last_updated, Some(ymd(2026, 3, 1)));
    assert_eq!(out[1].evidence_count, 1);
    assert_eq!(out[1].last_updated, Some(ymd(2026, 3, 20)));
}

#[test]
fn list_portfolios_is_exercised_with_a_real_summary_value() {
    // Smoke-tests the re-exported `PortfolioSummary` type by binding
    // to it and reading every field.
    let (vault, _store) = seeded_vault_with_one_portfolio();
    let out: Vec<PortfolioSummary> = vault.list_portfolios(ymd(2026, 4, 1)).unwrap();
    assert_eq!(out.len(), 1);
    let p = &out[0];
    let _ = (
        &p.slug,
        &p.question,
        p.evidence_count,
        p.last_updated,
        p.staleness_days,
    );
}

#[test]
fn get_portfolio_contents_returns_evidence_sorted_most_recent_first() {
    let (vault, _store) = seeded_vault_with_one_portfolio();
    vault
        .file_evidence(
            dt(2026, 3, 1, 9, 0),
            "sparse-vs-dense",
            "Earlier",
            "projects/foo",
            "x",
        )
        .unwrap();
    vault
        .file_evidence(
            dt(2026, 3, 22, 9, 0),
            "sparse-vs-dense",
            "Latest",
            "projects/foo",
            "y",
        )
        .unwrap();
    vault
        .file_evidence(
            dt(2026, 3, 10, 9, 0),
            "sparse-vs-dense",
            "Middle",
            "projects/foo",
            "z",
        )
        .unwrap();

    let out = vault.get_portfolio_contents("sparse-vs-dense").unwrap();
    let sources: Vec<&str> = out.iter().map(|(_, e)| e.source.as_str()).collect();
    assert_eq!(sources, vec!["Latest", "Middle", "Earlier"]);
}

#[test]
fn get_portfolio_contents_filters_out_other_portfolios() {
    let portfolio_other =
        "---\ntype: portfolio\nquestion: \"O\"\ncreated: 2026-02-01\nproject: null\n---\n# O\n";
    let (vault, _store) = vault_with_seeded_store(&[
        (
            "portfolios/sparse-vs-dense/_index.md",
            "---\ntype: portfolio\nquestion: \"A\"\ncreated: 2026-02-01\nproject: null\n---\n# A\n",
        ),
        ("portfolios/other/_index.md", portfolio_other),
    ]);
    vault
        .file_evidence(
            dt(2026, 3, 1, 9, 0),
            "sparse-vs-dense",
            "Mine",
            "projects/foo",
            "x",
        )
        .unwrap();
    vault
        .file_evidence(dt(2026, 3, 2, 9, 0), "other", "Theirs", "projects/foo", "y")
        .unwrap();

    let out = vault.get_portfolio_contents("sparse-vs-dense").unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].1.source, "Mine");
}

#[test]
fn get_portfolio_contents_is_empty_for_missing_portfolio() {
    let (vault, _store) = seeded_vault_with_one_portfolio();
    let out = vault.get_portfolio_contents("ghost").unwrap();
    assert!(out.is_empty());
}

#[test]
fn portfolio_not_found_lists_available_portfolios() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_portfolio(dt(2026, 2, 1, 9, 0), "Sparse models", None)
        .unwrap();
    vault
        .create_portfolio(dt(2026, 2, 1, 9, 0), "Dense models", None)
        .unwrap();

    let err = vault.get_portfolio("missing").unwrap_err();
    let DomainError::Store(StoreError::NotFound(msg)) = err else {
        panic!("expected Store(NotFound), got {err:?}");
    };
    assert!(
        msg.ends_with("available portfolios: dense-models, sparse-models"),
        "got: {msg}"
    );
}

#[test]
fn file_evidence_not_found_lists_available_portfolios() {
    // The write path (file_to_portfolio MCP tool) also names the valid set.
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_portfolio(dt(2026, 2, 1, 9, 0), "Sparse models", None)
        .unwrap();

    let err = vault
        .file_evidence(
            dt(2026, 3, 15, 10, 0),
            "ghost",
            "Source",
            "projects/foo",
            "body",
        )
        .unwrap_err();
    let DomainError::Store(StoreError::NotFound(msg)) = err else {
        panic!("expected Store(NotFound), got {err:?}");
    };
    assert!(
        msg.contains("available portfolios: sparse-models"),
        "got: {msg}"
    );
}

// ---------------------------------------------------------------------
// file_attachment — non-markdown evidence (#154)
// ---------------------------------------------------------------------

fn portfolio_seed() -> (&'static str, &'static str) {
    (
        "portfolios/sparse-vs-dense/_index.md",
        "---\ntype: portfolio\nquestion: \"Sparse vs dense\"\ncreated: 2026-02-01\nproject: null\n---\n\n# Sparse vs dense\n",
    )
}

#[test]
fn file_attachment_writes_flat_stub_and_imports_artefact() {
    let dir = tempfile::tempdir().unwrap();
    let artefact = dir.path().join("derivation.pdf");
    std::fs::write(&artefact, b"%PDF-1.7 fake bytes").unwrap();
    let (vault, store) = vault_with_seeded_store(&[portfolio_seed()]);

    let stub = vault
        .file_attachment(
            dt(2026, 6, 13, 10, 0),
            "sparse-vs-dense",
            &artefact,
            "Chen derivation",
            "projects/surrogate-model",
            "The closed-form bound for sparse attention.",
        )
        .expect("attach succeeds");

    // Stub flat at <portfolio>/<date>-<slug>.md; artefact in sibling folder.
    assert_eq!(
        stub,
        vp("portfolios/sparse-vs-dense/2026-06-13-chen-derivation.md")
    );
    assert!(
        store
            .exists(&vp(
                "portfolios/sparse-vs-dense/2026-06-13-chen-derivation/derivation.pdf"
            ))
            .unwrap(),
        "artefact imported into the sibling folder, original filename kept"
    );

    let raw = store.read_file(&stub).unwrap();
    assert!(raw.contains("kind: pdf"), "stub:\n{raw}");
    assert!(
        raw.contains("[derivation.pdf](<./2026-06-13-chen-derivation/derivation.pdf>)"),
        "relative angle-bracket link:\n{raw}"
    );
    assert!(raw.contains("The closed-form bound"), "abstract:\n{raw}");

    // Parses as evidence with the kind set; portfolio field intact.
    let fm = read_evidence_frontmatter(&store, &stub);
    assert_eq!(fm.kind.as_deref(), Some("pdf"));
    assert_eq!(fm.portfolio, "sparse-vs-dense");
    assert_eq!(fm.origin, "[[projects/surrogate-model]]");
}

#[test]
fn file_attachment_infers_kind_from_extension() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("whiteboard.png");
    std::fs::write(&png, b"\x89PNG fake").unwrap();
    let (vault, store) = vault_with_seeded_store(&[portfolio_seed()]);

    let stub = vault
        .file_attachment(
            dt(2026, 6, 13, 10, 0),
            "sparse-vs-dense",
            &png,
            "Whiteboard photo",
            "projects/surrogate-model",
            "",
        )
        .unwrap();
    let fm = read_evidence_frontmatter(&store, &stub);
    assert_eq!(fm.kind.as_deref(), Some("image"));
}

#[test]
fn file_attachment_uses_placeholder_for_empty_abstract() {
    let dir = tempfile::tempdir().unwrap();
    let art = dir.path().join("clip.mp4");
    std::fs::write(&art, b"fake").unwrap();
    let (vault, store) = vault_with_seeded_store(&[portfolio_seed()]);

    let stub = vault
        .file_attachment(
            dt(2026, 6, 13, 10, 0),
            "sparse-vs-dense",
            &art,
            "Screen recording",
            "projects/surrogate-model",
            "   ",
        )
        .unwrap();
    let raw = store.read_file(&stub).unwrap();
    assert!(raw.contains("kind: video"), "{raw}");
    assert!(raw.contains("Abstract pending"), "placeholder:\n{raw}");
}

#[test]
fn file_attachment_errors_when_portfolio_missing_with_hint() {
    let dir = tempfile::tempdir().unwrap();
    let art = dir.path().join("x.pdf");
    std::fs::write(&art, b"fake").unwrap();
    let (vault, _store) = vault_with_seeded_store(&[portfolio_seed()]);

    let err = vault
        .file_attachment(
            dt(2026, 6, 13, 10, 0),
            "ghost",
            &art,
            "X",
            "projects/foo",
            "",
        )
        .unwrap_err();
    let DomainError::Store(StoreError::NotFound(msg)) = err else {
        panic!("expected NotFound, got {err:?}");
    };
    assert!(
        msg.contains("available portfolios: sparse-vs-dense"),
        "{msg}"
    );
}

#[test]
fn file_attachment_errors_on_slug_collision() {
    let dir = tempfile::tempdir().unwrap();
    let art = dir.path().join("a.pdf");
    std::fs::write(&art, b"fake").unwrap();
    let (vault, _store) = vault_with_seeded_store(&[portfolio_seed()]);

    vault
        .file_attachment(
            dt(2026, 6, 13, 10, 0),
            "sparse-vs-dense",
            &art,
            "Dup",
            "projects/foo",
            "",
        )
        .unwrap();
    // Same day + same source ⇒ same evidence slug ⇒ stub already exists.
    let err = vault
        .file_attachment(
            dt(2026, 6, 13, 11, 0),
            "sparse-vs-dense",
            &art,
            "Dup",
            "projects/foo",
            "",
        )
        .unwrap_err();
    assert!(matches!(
        err,
        DomainError::Store(StoreError::AlreadyExists(_))
    ));
}

#[test]
fn file_attachment_escapes_a_quote_in_source() {
    let dir = tempfile::tempdir().unwrap();
    let art = dir.path().join("d.pdf");
    std::fs::write(&art, b"fake").unwrap();
    let (vault, store) = vault_with_seeded_store(&[portfolio_seed()]);

    // A double-quote in `source` must not break the YAML frontmatter.
    let stub = vault
        .file_attachment(
            dt(2026, 6, 13, 10, 0),
            "sparse-vs-dense",
            &art,
            "Chen \"GOAT\" 2025",
            "projects/foo",
            "",
        )
        .unwrap();
    // Round-trips through the parser, quote intact (would panic on a
    // malformed-frontmatter parse error if unescaped).
    let fm = read_evidence_frontmatter(&store, &stub);
    assert_eq!(fm.source, "Chen \"GOAT\" 2025");
    assert_eq!(fm.kind.as_deref(), Some("pdf"));
}

#[test]
fn file_attachment_escapes_angle_brackets_in_the_filename_link() {
    let dir = tempfile::tempdir().unwrap();
    let art = dir.path().join("a>b.pdf");
    std::fs::write(&art, b"fake").unwrap();
    let (vault, store) = vault_with_seeded_store(&[portfolio_seed()]);

    let stub = vault
        .file_attachment(
            dt(2026, 6, 13, 10, 0),
            "sparse-vs-dense",
            &art,
            "Odd name",
            "projects/foo",
            "",
        )
        .unwrap();
    let raw = store.read_file(&stub).unwrap();
    // The `>` in the angle-bracket link destination is backslash-escaped
    // so it doesn't terminate the destination early.
    assert!(raw.contains("a\\>b.pdf>)"), "escaped link dest:\n{raw}");
}
