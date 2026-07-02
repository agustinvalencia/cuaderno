//! Tests for `Vault::search` — query sanitisation and the note-type,
//! date-range, and portfolio filters layered over the core FTS index.
//!
//! Notes are seeded as raw files and indexed through `Vault::new`'s
//! reconciliation (which populates the FTS rows), so these exercise the
//! real domain path against the in-memory index double.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::{SearchFilters, Vault};
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Minimal indexable note: `type` frontmatter, an H1 title, a body.
fn note(note_type: &str, title: &str, body: &str) -> String {
    format!("---\ntype: {note_type}\n---\n# {title}\n\n{body}\n")
}

/// Same, with extra frontmatter lines (e.g. `created:`, `portfolio:`).
fn note_with(note_type: &str, extra: &str, title: &str, body: &str) -> String {
    format!("---\ntype: {note_type}\n{extra}---\n# {title}\n\n{body}\n")
}

fn vault_with(notes: &[(&str, String)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, content) in notes {
        store.write_file(&vp(path), content).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

/// Same as [`vault_with`] but with a caller-supplied config — used to
/// exercise the `ignore` list's effect on search.
fn vault_with_config(notes: &[(&str, String)], config: VaultConfig) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, content) in notes {
        store.write_file(&vp(path), content).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, config).expect("Vault::new");
    vault
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[test]
fn search_does_not_surface_an_ignored_note() {
    // Two notes share a unique term; one path is in `ignore`. Search
    // must return only the un-ignored note — proving the config `ignore`
    // exclusion reaches search (its FTS row was never indexed), not just
    // the reconciler.
    let config = VaultConfig {
        ignore: vec!["inbox/secret.md".to_string()],
        ..Default::default()
    };
    let vault = vault_with_config(
        &[
            (
                "inbox/secret.md",
                note("inbox", "Secret", "zentangle marginalia"),
            ),
            (
                "inbox/public.md",
                note("inbox", "Public", "zentangle marginalia"),
            ),
        ],
        config,
    );

    let hits = vault
        .search("zentangle", &SearchFilters::default(), 10)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("inbox/public.md"));
}

#[test]
fn finds_a_note_by_a_body_term() {
    let vault = vault_with(&[(
        "inbox/capture.md",
        note("inbox", "Capture", "remember the parking permit"),
    )]);

    let hits = vault
        .search("permit", &SearchFilters::default(), 10)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("inbox/capture.md"));
    assert_eq!(hits[0].note_type, "inbox");
}

#[test]
fn is_case_insensitive() {
    let vault = vault_with(&[("inbox/a.md", note("inbox", "A", "Quarterly Budget review"))]);
    assert_eq!(
        vault
            .search("BUDGET", &SearchFilters::default(), 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn sanitises_stray_quotes_rather_than_erroring() {
    let vault = vault_with(&[("inbox/a.md", note("inbox", "A", "the venue is booked"))]);

    // An unbalanced quote would be an invalid raw FTS5 MATCH; the
    // sanitiser must turn it into a valid query, not an error.
    let hits = vault
        .search("\"venue", &SearchFilters::default(), 10)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("inbox/a.md"));
}

#[test]
fn blank_or_punctuation_only_query_returns_no_results() {
    let vault = vault_with(&[("inbox/a.md", note("inbox", "A", "some content"))]);
    assert!(
        vault
            .search("   ", &SearchFilters::default(), 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        vault
            .search("!!! ---", &SearchFilters::default(), 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn multi_term_query_requires_all_terms() {
    let vault = vault_with(&[
        ("inbox/a.md", note("inbox", "A", "budget meeting tomorrow")),
        ("inbox/b.md", note("inbox", "B", "budget notes only")),
    ]);

    // Terms are ANDed: only the note containing both words matches.
    let hits = vault
        .search("budget meeting", &SearchFilters::default(), 10)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("inbox/a.md"));
}

#[test]
fn note_type_filter_restricts_results() {
    let vault = vault_with(&[
        ("projects/p.md", note("project", "P", "alpha strategy")),
        (
            "questions/research/q.md",
            note("question", "Q", "alpha question"),
        ),
    ]);

    let filters = SearchFilters {
        note_type_names: vec!["question".to_owned()],
        ..Default::default()
    };
    let hits = vault.search("alpha", &filters, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].note_type, "question");
}

#[test]
fn limit_caps_result_count() {
    let vault = vault_with(&[
        ("inbox/a.md", note("inbox", "A", "shared alpha term")),
        ("inbox/b.md", note("inbox", "B", "shared alpha term")),
        ("inbox/c.md", note("inbox", "C", "shared alpha term")),
    ]);
    assert_eq!(
        vault
            .search("alpha", &SearchFilters::default(), 2)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        vault
            .search("alpha", &SearchFilters::default(), 10)
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn portfolio_filter_matches_frontmatter() {
    let vault = vault_with(&[
        (
            "portfolios/alpha/e1.md",
            note_with("evidence", "portfolio: alpha\n", "E1", "a key finding"),
        ),
        (
            "portfolios/beta/e2.md",
            note_with("evidence", "portfolio: beta\n", "E2", "another finding"),
        ),
    ]);

    let filters = SearchFilters {
        portfolio: Some("alpha".to_owned()),
        ..Default::default()
    };
    let hits = vault.search("finding", &filters, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("portfolios/alpha/e1.md"));
}

#[test]
fn date_range_filters_by_created_frontmatter() {
    let vault = vault_with(&[
        (
            "inbox/jan.md",
            note_with("inbox", "created: 2026-01-10\n", "Jan", "alpha capture"),
        ),
        (
            "inbox/mar.md",
            note_with("inbox", "created: 2026-03-10\n", "Mar", "alpha capture"),
        ),
    ]);

    let filters = SearchFilters {
        date_from: Some(date(2026, 2, 1)),
        ..Default::default()
    };
    let hits = vault.search("alpha", &filters, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("inbox/mar.md"));
}

#[test]
fn a_note_with_an_unparseable_created_is_treated_as_undated_not_fatal() {
    let vault = vault_with(&[
        (
            "inbox/good.md",
            note_with("inbox", "created: 2026-03-10\n", "Good", "alpha capture"),
        ),
        (
            "inbox/bad.md",
            note_with("inbox", "created: someday\n", "Bad", "alpha capture"),
        ),
    ]);

    // The malformed `created` is treated as undated (excluded by the date
    // bound), and the search still succeeds for the good note.
    let filters = SearchFilters {
        date_from: Some(date(2026, 2, 1)),
        ..Default::default()
    };
    let hits = vault.search("alpha", &filters, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("inbox/good.md"));
}

#[test]
fn a_non_daily_note_named_like_a_date_is_dated_by_created_not_its_filename() {
    // The filename-date shortcut is gated to daily notes; an inbox note
    // that merely happens to be named YYYY-MM-DD takes its `created`.
    let vault = vault_with(&[(
        "inbox/2026-02-15.md",
        note_with(
            "inbox",
            "created: 2026-09-01\n",
            "Misnamed",
            "alpha capture",
        ),
    )]);

    // Window matches the `created` (September), not the filename (February).
    let filters = SearchFilters {
        date_from: Some(date(2026, 8, 1)),
        ..Default::default()
    };
    let hits = vault.search("alpha", &filters, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("inbox/2026-02-15.md"));
}

#[test]
fn date_range_uses_the_daily_note_filename_date() {
    let vault = vault_with(&[
        (
            "journal/2026/daily/2026-02-15.md",
            note("daily", "2026-02-15", "alpha standup"),
        ),
        (
            "journal/2026/daily/2026-05-15.md",
            note("daily", "2026-05-15", "alpha standup"),
        ),
    ]);

    let filters = SearchFilters {
        date_from: Some(date(2026, 2, 1)),
        date_to: Some(date(2026, 2, 28)),
        ..Default::default()
    };
    let hits = vault.search("alpha", &filters, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("journal/2026/daily/2026-02-15.md"));
}

#[test]
fn note_type_filter_matches_a_config_custom_type() {
    // The string filter matches config-defined custom types too.
    use cdno_core::config::CustomNoteType;
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "person".to_owned(),
        CustomNoteType {
            folder: "people".to_owned(),
            required: vec![],
            optional: vec![],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    let vault = vault_with_config(
        &[
            ("people/ada.md", note("person", "Ada", "sparse alpha")),
            ("projects/p.md", note("project", "P", "sparse alpha")),
        ],
        config,
    );

    let filters = SearchFilters {
        note_type_names: vec!["person".to_owned()],
        ..Default::default()
    };
    let hits = vault.search("sparse", &filters, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].note_type, "person");
}

#[test]
fn custom_type_filter_with_no_matching_notes_is_empty() {
    // A registered custom type with no notes of that type returns no hits
    // (the domain filter is lenient; the CLI/MCP validate the name).
    use cdno_core::config::CustomNoteType;
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "person".to_owned(),
        CustomNoteType {
            folder: "people".to_owned(),
            required: vec![],
            optional: vec![],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    let vault = vault_with_config(
        &[("projects/p.md", note("project", "P", "sparse alpha"))],
        config,
    );
    let filters = SearchFilters {
        note_type_names: vec!["person".to_owned()],
        ..Default::default()
    };
    assert!(vault.search("sparse", &filters, 10).unwrap().is_empty());
}
