//! FTS5 full-text search behaviour on `SqliteIndex` (#172): ranking,
//! snippets, porter stemming, removal, and the transaction commit seam
//! that keeps the FTS index live on every write.

use std::sync::Arc;

use cdno_core::index::{NoteEntry, SqliteIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_core::transaction::VaultTransaction;
use serde_json::json;
use tempfile::TempDir;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn store() -> (TempDir, SqliteIndex) {
    let dir = TempDir::new().unwrap();
    let index = SqliteIndex::open(dir.path().join("index.sqlite")).unwrap();
    (dir, index)
}

fn note(path: &str, note_type: &str, title: &str) -> NoteEntry {
    NoteEntry {
        path: vp(path),
        note_type: note_type.to_owned(),
        title: Some(title.to_owned()),
        content_hash: "h".to_owned(),
        mtime_ns: 1,
        size: 1,
        frontmatter: json!({}),
        indexed_at_ns: 1,
    }
}

/// Index a note as both a `notes` row (so the search JOIN resolves
/// `note_type`) and an FTS row.
fn index_note(idx: &SqliteIndex, path: &str, note_type: &str, title: &str, body: &str) {
    idx.upsert_note(&note(path, note_type, title)).unwrap();
    idx.replace_fts(&vp(path), Some(title), body).unwrap();
}

#[test]
fn search_returns_hit_with_bracketed_snippet() {
    let (_d, idx) = store();
    index_note(
        &idx,
        "journal/daily/2026-06-11.md",
        "daily",
        "Daily",
        "the quarterly budget was approved at standup",
    );

    let hits = idx.search("budget", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("journal/daily/2026-06-11.md"));
    assert_eq!(hits[0].note_type, "daily");
    // snippet() wraps the matched term in the configured markers.
    assert!(
        hits[0].snippet.contains("[budget]"),
        "snippet missing bracketed term: {}",
        hits[0].snippet
    );
}

#[test]
fn title_hit_outranks_body_only_hit() {
    let (_d, idx) = store();
    // A: the query word is the title. B: it appears once in the body.
    index_note(
        &idx,
        "projects/a.md",
        "project",
        "Budget planning",
        "misc notes",
    );
    index_note(
        &idx,
        "projects/b.md",
        "project",
        "Misc",
        "we mentioned the budget once in passing",
    );

    let hits = idx.search("budget", 10).unwrap();
    assert_eq!(hits.len(), 2);
    // bm25 weights title 10x body, and lower score sorts first.
    assert_eq!(hits[0].path, vp("projects/a.md"));
    assert!(
        hits[0].score < hits[1].score,
        "expected title hit to rank strictly better: {} vs {}",
        hits[0].score,
        hits[1].score
    );
}

#[test]
fn title_only_match_snippets_the_title() {
    let (_d, idx) = store();
    // Query word is in the title, not the body.
    index_note(
        &idx,
        "projects/a.md",
        "project",
        "Budget planning",
        "misc notes",
    );

    let hits = idx.search("budget", 10).unwrap();
    assert_eq!(hits.len(), 1);
    // snippet auto-selects the matched column, so the title term is
    // bracketed rather than returning an unrelated body prefix.
    assert!(
        hits[0].snippet.contains("[Budget]"),
        "title-only hit should snippet the title: {}",
        hits[0].snippet
    );
}

#[test]
fn malformed_query_errors_rather_than_panicking() {
    let (_d, idx) = store();
    index_note(&idx, "a.md", "inbox", "A", "some body text");

    // An unbalanced quote is an invalid FTS5 MATCH expression. It must
    // surface as an Err (sanitising/translation is a higher-layer job),
    // never a panic.
    let result = idx.search("\"unbalanced", 10);
    assert!(result.is_err(), "malformed query should return Err");
}

#[test]
fn porter_stemmer_matches_inflections() {
    let (_d, idx) = store();
    index_note(
        &idx,
        "inbox/note.md",
        "inbox",
        "Note",
        "we held three meetings about the roadmap",
    );

    // Singular query matches the plural in the body via the porter stem.
    let hits = idx.search("meeting", 10).unwrap();
    assert_eq!(
        hits.len(),
        1,
        "porter stemming should match meetings/meeting"
    );
    assert_eq!(hits[0].path, vp("inbox/note.md"));
}

#[test]
fn phrase_query_requires_adjacency() {
    let (_d, idx) = store();
    index_note(&idx, "a.md", "inbox", "A", "budget meeting tomorrow");
    index_note(
        &idx,
        "b.md",
        "inbox",
        "B",
        "the budget is fine, no meeting yet",
    );

    // Quoted phrase only matches adjacent terms.
    let hits = idx.search("\"budget meeting\"", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("a.md"));
}

#[test]
fn remove_note_drops_it_from_search() {
    let (_d, idx) = store();
    index_note(&idx, "x.md", "inbox", "X", "findable text here");
    assert_eq!(idx.search("findable", 10).unwrap().len(), 1);

    idx.remove_note(&vp("x.md")).unwrap();
    assert!(idx.search("findable", 10).unwrap().is_empty());
}

#[test]
fn replace_fts_overwrites_prior_body() {
    let (_d, idx) = store();
    index_note(&idx, "x.md", "inbox", "X", "the old contents");
    assert_eq!(idx.search("old", 10).unwrap().len(), 1);

    idx.replace_fts(&vp("x.md"), Some("X"), "the new contents")
        .unwrap();
    assert!(
        idx.search("old", 10).unwrap().is_empty(),
        "stale body should be gone after replace_fts"
    );
    assert_eq!(idx.search("new", 10).unwrap().len(), 1);
}

#[test]
fn limit_caps_result_count() {
    let (_d, idx) = store();
    for i in 0..3 {
        index_note(
            &idx,
            &format!("n{i}.md"),
            "inbox",
            "N",
            "shared alpha keyword",
        );
    }
    assert_eq!(idx.search("alpha", 2).unwrap().len(), 2);
    assert_eq!(idx.search("alpha", 10).unwrap().len(), 3);
}

#[test]
fn fts_indexed_paths_lists_every_fts_row() {
    let (_d, idx) = store();
    index_note(&idx, "a.md", "inbox", "A", "one");
    index_note(&idx, "b.md", "inbox", "B", "two");

    let paths = idx.fts_indexed_paths().unwrap();
    assert_eq!(paths, vec![vp("a.md"), vp("b.md")]);
}

#[test]
fn commit_makes_a_written_note_searchable_without_reconcile() {
    // The transaction commit seam derives the FTS body from the paired
    // file write, so a note is searchable the instant it's committed —
    // no reconcile pass needed (the same-session liveness #172 wants).
    let dir = TempDir::new().unwrap();
    let index: Arc<SqliteIndex> =
        Arc::new(SqliteIndex::open(dir.path().join("index.sqlite")).unwrap());
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index_dyn: Arc<dyn VaultIndex> = index.clone();

    let path = vp("inbox/capture.md");
    let content =
        "---\ntype: inbox\ntitle: Capture\n---\n# Capture\n\nremember the parking permit\n";

    let mut tx = VaultTransaction::new(store.clone(), index_dyn.clone()).expect("write lock");
    tx.write_file(path.clone(), content);
    tx.upsert_note(note("inbox/capture.md", "inbox", "Capture"));
    tx.commit().unwrap();

    let hits = index.search("permit", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, path);
    // Frontmatter is stripped: a frontmatter-only term doesn't match body.
    assert!(
        index.search("parking", 10).unwrap().len() == 1,
        "body term should be searchable"
    );
}

#[test]
fn commit_sources_the_fts_title_from_the_body_h1() {
    // Notes carry their title as the H1, not a frontmatter field, so the
    // commit seam must lift the H1 into the weighted FTS title column for
    // the bm25 boost to mean anything. Both notes mention "budget"; only
    // A has it as its H1, so A must rank first.
    let dir = TempDir::new().unwrap();
    let index: Arc<SqliteIndex> =
        Arc::new(SqliteIndex::open(dir.path().join("index.sqlite")).unwrap());
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index_dyn: Arc<dyn VaultIndex> = index.clone();

    let write = |p: &str, content: &str| {
        let mut tx = VaultTransaction::new(store.clone(), index_dyn.clone()).expect("write lock");
        tx.write_file(vp(p), content);
        tx.upsert_note(note(p, "project", "ignored")); // entry.title is unused for FTS
        tx.commit().unwrap();
    };
    write(
        "projects/a.md",
        "---\ntype: project\n---\n# Budget\n\nplanning notes\n",
    );
    write(
        "projects/b.md",
        "---\ntype: project\n---\n# Roadmap\n\nwe touched the budget once\n",
    );

    let hits = index.search("budget", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits[0].path,
        vp("projects/a.md"),
        "the note whose H1 is the query term should rank first"
    );
}
