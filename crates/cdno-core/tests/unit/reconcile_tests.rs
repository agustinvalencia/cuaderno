use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cdno_core::config::IgnoreSet;
use cdno_core::error::{IndexError, StoreError};
use cdno_core::file_meta::FileMeta;
use cdno_core::hash::content_hash;
use cdno_core::index::{
    DeadlineEntry, LinkEntry, MemoryIndex, MilestoneEntry, NoteEntry, VaultIndex,
};
use cdno_core::path::VaultPath;
use cdno_core::reconcile::reconcile;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_core::transaction::VaultTransaction;
use serde_json::json;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn seed_note(store: &MemoryVaultStore, path: &str, note_type: &str, extra: &str) {
    let raw = format!(
        "---\ntype: {note_type}\ntitle: {path} title\n{extra}---\n# Body\n\ncontent for {path}\n"
    );
    store.write_file(&vp(path), &raw).unwrap();
}

fn fixtures() -> (Arc<MemoryVaultStore>, Arc<MemoryIndex>) {
    (
        Arc::new(MemoryVaultStore::new()),
        Arc::new(MemoryIndex::new()),
    )
}

fn as_store(s: &Arc<MemoryVaultStore>) -> Arc<dyn VaultStore> {
    s.clone()
}
fn as_index(i: &Arc<MemoryIndex>) -> Arc<dyn VaultIndex> {
    i.clone()
}

#[test]
fn empty_vault_empty_index_nothing_happens() {
    let (store, index) = fixtures();
    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 0);
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.removed, 0);
    assert!(report.errors.is_empty());
}

#[test]
fn new_file_is_added() {
    let (store, index) = fixtures();
    seed_note(&store, "journal/daily/2026-04-19.md", "daily", "");

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.added, 1);
    assert_eq!(report.updated, 0);
    assert_eq!(report.removed, 0);

    let entry = index
        .find_by_path(&vp("journal/daily/2026-04-19.md"))
        .unwrap()
        .unwrap();
    assert_eq!(entry.note_type, "daily");
    assert_eq!(
        entry.title.as_deref(),
        Some("journal/daily/2026-04-19.md title")
    );
}

#[test]
fn reconcile_indexes_inline_body_tags_alongside_frontmatter_tags() {
    let (store, index) = fixtures();
    let raw = "---\ntype: daily\ntitle: Mixed tags\ntags:\n  - frontmatter-tag\n---\n\n\
        a body with #inline-tag and another #shared\n";
    store
        .write_file(&vp("note.md"), raw)
        .expect("seed mixed-tags note");

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 1);

    let frontmatter_hits = index.find_by_tag("frontmatter-tag").unwrap();
    let inline_hits = index.find_by_tag("inline-tag").unwrap();
    let shared_hits = index.find_by_tag("shared").unwrap();
    assert_eq!(frontmatter_hits, vec![vp("note.md")]);
    assert_eq!(inline_hits, vec![vp("note.md")]);
    assert_eq!(shared_hits, vec![vp("note.md")]);
}

#[test]
fn reconcile_indexes_namespaced_action_tags() {
    // The Faraday-style query (design §5.11): a daily entry mentioning
    // `#action/<slug>` must be findable by the full namespaced tag, not
    // truncated to its `action` prefix.
    let (store, index) = fixtures();
    let raw = "---\ntype: daily\ntitle: Train of thought\n---\n\n\
        tried a new loss today, see #action/characterise-sample-efficiency\n";
    store
        .write_file(&vp("journal/daily/2026-05-26.md"), raw)
        .expect("seed action-tagged daily note");

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 1);

    let full = index
        .find_by_tag("action/characterise-sample-efficiency")
        .unwrap();
    assert_eq!(full, vec![vp("journal/daily/2026-05-26.md")]);
    // The slug isn't dropped: a bare `action` prefix doesn't match.
    assert!(index.find_by_tag("action").unwrap().is_empty());
}

#[test]
fn reconcile_resolves_wikilinks_to_their_target_paths() {
    let (store, index) = fixtures();
    seed_note(&store, "notes/foo.md", "daily", "");
    seed_note(&store, "src.md", "daily", "");
    // Manually overwrite `src.md` with body that contains wikilinks.
    let raw = "---\ntype: daily\n---\n\n\
        unique basename: [[foo]]\n\
        unresolved: [[never-existed]]\n\
        with label: [[foo|My Foo]]\n";
    store.write_file(&vp("src.md"), raw).unwrap();

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    let outgoing = index.find_outgoing_links(&vp("src.md")).unwrap();
    assert_eq!(outgoing.len(), 3, "got: {outgoing:?}");

    let foo_resolved = outgoing
        .iter()
        .find(|e| e.target_raw == "foo" && e.label.is_none())
        .expect("plain `foo` link");
    assert_eq!(
        foo_resolved.resolved_path.as_ref(),
        Some(&vp("notes/foo.md"))
    );

    let unresolved = outgoing
        .iter()
        .find(|e| e.target_raw == "never-existed")
        .expect("unresolved link recorded");
    assert!(unresolved.resolved_path.is_none());

    let labelled = outgoing
        .iter()
        .find(|e| e.label.as_deref() == Some("My Foo"))
        .expect("labelled link");
    assert_eq!(labelled.target_raw, "foo");
    assert_eq!(labelled.resolved_path.as_ref(), Some(&vp("notes/foo.md")));
}

#[test]
fn reconcile_backlinks_a_relocated_note_via_last_segment_fallback() {
    // #215 end-to-end: a daily references `[[actions/<slug>]]` while the
    // note lives at `actions/_done/<year>/<slug>.md`. After reconcile,
    // the archived note's backlinks include the daily — the last-segment
    // fallback feeds the backlink graph, not only the lint.
    let (store, index) = fixtures();
    seed_note(&store, "actions/_done/2026/characterise.md", "action", "");
    let daily = "---\ntype: daily\n---\n\nfollow-up on [[actions/characterise]]\n";
    store
        .write_file(&vp("journal/2026/daily/2026-05-02.md"), daily)
        .unwrap();

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    let backlinks = index
        .find_backlinks(&vp("actions/_done/2026/characterise.md"))
        .unwrap();
    assert!(
        backlinks.contains(&vp("journal/2026/daily/2026-05-02.md")),
        "the archived action should be backlinked from the daily: {backlinks:?}"
    );
}

#[test]
fn reconcile_backlinks_a_frontmatter_wikilink() {
    // #395 end-to-end: a project's `core_question:` is a FRONTMATTER
    // wikilink (its body has none). After reconcile the question is
    // backlinked from the project — the frontmatter scan feeds the backlink
    // graph at the reconcile layer, not only body links.
    let (store, index) = fixtures();
    let question = "---\ntype: question\ndomain: research\n---\n\n# q?\n";
    // No body wikilink anywhere — the only link is in the frontmatter.
    let project =
        "---\ntype: project\ncore_question: \"[[questions/research/q]]\"\n---\n\n# Surrogate\n";
    store
        .write_file(&vp("questions/research/q.md"), question)
        .unwrap();
    store
        .write_file(&vp("projects/surrogate.md"), project)
        .unwrap();

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    let backlinks = index
        .find_backlinks(&vp("questions/research/q.md"))
        .unwrap();
    assert!(
        backlinks.contains(&vp("projects/surrogate.md")),
        "the question should be backlinked from the project's core_question: {backlinks:?}"
    );
}

#[test]
fn reconcile_marks_wikilink_unresolved_when_basename_is_ambiguous() {
    let (store, index) = fixtures();
    seed_note(&store, "a/foo.md", "daily", "");
    seed_note(&store, "b/foo.md", "daily", "");
    let raw = "---\ntype: daily\n---\n\nbody references [[foo]]\n";
    store.write_file(&vp("src.md"), raw).unwrap();

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    let outgoing = index.find_outgoing_links(&vp("src.md")).unwrap();
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].target_raw, "foo");
    assert!(
        outgoing[0].resolved_path.is_none(),
        "ambiguous basename must not resolve: {:?}",
        outgoing[0].resolved_path,
    );
}

#[test]
fn cuaderno_meta_files_are_excluded_from_scan() {
    let (store, index) = fixtures();
    // A real note at vault root.
    seed_note(&store, "journal/daily/2026-04-19.md", "daily", "");
    // A markdown file under the meta directory — must be invisible
    // to the indexer even though its extension and frontmatter type
    // would otherwise match a real note.
    seed_note(&store, ".cuaderno/templates/daily.md", "daily", "");

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    assert_eq!(report.scanned, 1, "only the real note should be scanned");
    assert_eq!(report.added, 1);
    assert!(
        index
            .find_by_path(&vp(".cuaderno/templates/daily.md"))
            .unwrap()
            .is_none(),
        "meta files must not appear in the index"
    );
}

#[test]
fn stale_cuaderno_index_row_is_removed_as_orphan() {
    // Simulates upgrading from a buggy version: an old index row
    // points at a `.cuaderno/templates/*.md` file that's still on
    // disk. Reconciliation now skips the file during the walk, so
    // the row has no fs counterpart and must be cleaned up.
    let (store, index) = fixtures();
    seed_note(&store, ".cuaderno/templates/daily.md", "daily", "");
    index
        .upsert_note(&NoteEntry {
            path: vp(".cuaderno/templates/daily.md"),
            note_type: "daily".into(),
            title: None,
            content_hash: "stale".into(),
            mtime_ns: 0,
            size: 0,
            frontmatter: json!({}),
            indexed_at_ns: 0,
        })
        .unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    assert_eq!(report.scanned, 0);
    assert_eq!(report.removed, 1);
    assert!(
        index
            .find_by_path(&vp(".cuaderno/templates/daily.md"))
            .unwrap()
            .is_none(),
    );
}

#[test]
fn matching_index_entry_is_skipped() {
    let (store, index) = fixtures();
    seed_note(&store, "note.md", "daily", "");
    // First run indexes the note.
    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    // Second run sees the same state — no adds/updates.
    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.removed, 0);
}

#[test]
fn changed_content_triggers_update() {
    let (store, index) = fixtures();
    seed_note(&store, "note.md", "daily", "");
    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    // Rewrite with different content → hash changes.
    store
        .write_file(
            &vp("note.md"),
            "---\ntype: daily\ntitle: updated\n---\n# Body\nnew content\n",
        )
        .unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 1);
    assert_eq!(report.removed, 0);

    let entry = index.find_by_path(&vp("note.md")).unwrap().unwrap();
    assert_eq!(entry.title.as_deref(), Some("updated"));
}

#[test]
fn orphan_index_rows_are_removed() {
    let (store, index) = fixtures();
    seed_note(&store, "keep.md", "daily", "");
    seed_note(&store, "delete-me.md", "daily", "");
    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    // Simulate external deletion of delete-me.md.
    store.delete_file(&vp("delete-me.md")).unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.removed, 1);
    assert!(index.find_by_path(&vp("delete-me.md")).unwrap().is_none());
    assert!(index.find_by_path(&vp("keep.md")).unwrap().is_some());
}

#[test]
fn non_markdown_files_are_ignored() {
    let (store, index) = fixtures();
    seed_note(&store, "note.md", "daily", "");
    // PDFs and .ipynb files in evidence folders must not be parsed.
    store
        .write_file(&vp("portfolios/foo/paper.pdf"), "%PDF-1.4\nbinary")
        .unwrap();
    store
        .write_file(&vp("portfolios/foo/notebook.ipynb"), "{\"cells\": []}")
        .unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.added, 1);
    assert!(report.errors.is_empty());
    assert!(
        index
            .find_by_path(&vp("portfolios/foo/paper.pdf"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn note_without_frontmatter_is_reported_as_error() {
    let (store, index) = fixtures();
    store
        .write_file(&vp("broken.md"), "# No frontmatter here\nplain body\n")
        .unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.added, 0);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].path, vp("broken.md"));
}

#[test]
fn one_broken_note_does_not_block_others() {
    let (store, index) = fixtures();
    seed_note(&store, "good.md", "daily", "");
    store
        .write_file(&vp("bad.md"), "no frontmatter, just words\n")
        .unwrap();
    seed_note(&store, "also-good.md", "daily", "");

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 3);
    assert_eq!(report.added, 2);
    assert_eq!(report.errors.len(), 1);

    assert!(index.find_by_path(&vp("good.md")).unwrap().is_some());
    assert!(index.find_by_path(&vp("also-good.md")).unwrap().is_some());
    assert!(index.find_by_path(&vp("bad.md")).unwrap().is_none());
}

#[test]
fn project_note_milestones_populate_deadlines() {
    let (store, index) = fixtures();
    let project = "\
---
type: project
title: Surrogate Model
---
# Current State
working on it

# Milestones
- [ ] ship v1 — hard: 2026-05-01
- [x] baseline trained — hard: 2026-02-10
- [ ] full geometry eval — target: April
- [ ] ICML paper submitted — hard: 2026-05-22
";
    store
        .write_file(&vp("projects/surrogate.md"), project)
        .unwrap();

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    let deadlines = index.deadlines_between("2026-01-01", "2027-01-01").unwrap();
    let titles: Vec<String> = deadlines.iter().map(|(_, d)| d.title.clone()).collect();
    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"ship v1".to_owned()));
    assert!(titles.contains(&"ICML paper submitted".to_owned()));
}

#[test]
fn non_project_notes_do_not_get_deadlines() {
    let (store, index) = fixtures();
    let raw = "\
---
type: daily
title: 2026-04-19
---
# Milestones
- [ ] not a project milestone — hard: 2026-05-01
";
    store
        .write_file(&vp("journal/daily/2026-04-19.md"), raw)
        .unwrap();

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    let deadlines = index.deadlines_between("2026-01-01", "2027-01-01").unwrap();
    assert!(deadlines.is_empty(), "daily notes must not spawn deadlines");
}

#[test]
fn project_note_populates_full_milestone_timeline() {
    // The milestones table is a superset of the deadlines feed: it
    // carries soft targets and completed markers, not just hard
    // deadlines (#109).
    let (store, index) = fixtures();
    let raw = "\
---
type: project
title: Surrogate model
---
# Milestones
- [x] Baseline done — 2026-02-10
- [ ] Full geometry evaluation — target: April
- [ ] ICML paper submitted — hard: 2026-05-22
";
    store.write_file(&vp("projects/surrogate.md"), raw).unwrap();

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    // All three milestones land, in source order, queryable by slug.
    let all = index.milestones_for_project("surrogate").unwrap();
    let names: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Baseline done",
            "Full geometry evaluation",
            "ICML paper submitted",
        ],
    );
    assert!(all[0].completed);
    assert_eq!(
        all[1].date, None,
        "fuzzy `target: April` has no sortable date"
    );
    assert!(all[2].is_hard && !all[2].completed);

    // Only the two dated milestones appear in the date window; the
    // undated soft target is excluded.
    let dated = index
        .milestones_between("2026-01-01", "2026-12-31")
        .unwrap();
    let dated_names: Vec<&str> = dated.iter().map(|(_, m)| m.name.as_str()).collect();
    assert_eq!(dated_names, vec!["Baseline done", "ICML paper submitted"]);
}

#[test]
fn non_project_notes_do_not_get_milestones() {
    let (store, index) = fixtures();
    let raw = "\
---
type: daily
title: 2026-04-19
---
# Milestones
- [ ] not a project milestone — hard: 2026-05-01
";
    store
        .write_file(&vp("journal/daily/2026-04-19.md"), raw)
        .unwrap();

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    assert!(
        index
            .milestones_between("2026-01-01", "2027-01-01")
            .unwrap()
            .is_empty(),
        "daily notes must not spawn milestones",
    );
}

#[test]
fn frontmatter_tags_are_indexed() {
    let (store, index) = fixtures();
    let raw = "\
---
type: daily
title: 2026-04-19
tags:
  - agustin-valencia
  - deep-work
---
# Body
content
";
    store.write_file(&vp("note.md"), raw).unwrap();

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    let tagged = index.find_by_tag("agustin-valencia").unwrap();
    assert_eq!(tagged, vec![vp("note.md")]);
    let deep = index.find_by_tag("deep-work").unwrap();
    assert_eq!(deep, vec![vp("note.md")]);
}

#[test]
fn updating_a_project_replaces_its_deadlines() {
    let (store, index) = fixtures();
    store
        .write_file(
            &vp("projects/p.md"),
            "---\ntype: project\n---\n# Milestones\n- [ ] first — hard: 2026-05-01\n",
        )
        .unwrap();
    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    // Replace the milestone list.
    store
        .write_file(
            &vp("projects/p.md"),
            "---\ntype: project\n---\n# Milestones\n- [ ] second — hard: 2026-06-01\n",
        )
        .unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.updated, 1);

    let deadlines = index.deadlines_between("2026-01-01", "2027-01-01").unwrap();
    assert_eq!(deadlines.len(), 1);
    assert_eq!(deadlines[0].1.title, "second");
}

#[test]
fn removing_a_note_cascades_its_deadlines_and_tags() {
    let (store, index) = fixtures();
    let raw = "\
---
type: project
tags:
  - deep-work
---
# Milestones
- [ ] ship — hard: 2026-05-01
";
    store.write_file(&vp("projects/p.md"), raw).unwrap();
    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    store.delete_file(&vp("projects/p.md")).unwrap();
    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    assert!(
        index
            .deadlines_between("2026-01-01", "2027-01-01")
            .unwrap()
            .is_empty()
    );
    assert!(index.find_by_tag("deep-work").unwrap().is_empty());
}

#[test]
fn orphan_removal_failure_is_reported_as_error() {
    // Reconciliation records an error (and does not mark `removed`)
    // when the index refuses to drop the orphan row. The next pass
    // will retry; meanwhile the caller sees the failure.
    let (store, backing_index) = fixtures();
    seed_note(&store, "orphan.md", "daily", "");
    reconcile(
        &as_store(&store),
        &as_index(&backing_index),
        &IgnoreSet::empty(),
    )
    .unwrap();

    // Simulate the file being deleted on disk.
    store.delete_file(&vp("orphan.md")).unwrap();

    // Run reconciliation through a wrapper whose remove_note always
    // fails. The underlying index is still the same backing store.
    let failing: Arc<dyn VaultIndex> = Arc::new(FailOnRemoveIndex {
        inner: backing_index.clone(),
    });
    let store_arc: Arc<dyn VaultStore> = store.clone();
    let report = reconcile(&store_arc, &failing, &IgnoreSet::empty()).unwrap();

    assert_eq!(report.removed, 0);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].path, vp("orphan.md"));
    // Orphan still in the backing index — the next pass will try again.
    assert!(
        backing_index
            .find_by_path(&vp("orphan.md"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn reconcile_populates_fts_for_indexed_notes() {
    let (store, index) = fixtures();
    seed_note(&store, "inbox/a.md", "inbox", "");
    seed_note(&store, "projects/b.md", "project", "status: active\n");

    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    // reconcile_one wrote the FTS rows alongside the note rows, so the
    // body text ("content for <path>") is searchable.
    let hits = index.search("content", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(
        index.fts_indexed_paths().unwrap(),
        vec![vp("inbox/a.md"), vp("projects/b.md")]
    );
}

#[test]
fn reconcile_backfills_fts_for_notes_missing_from_search() {
    use cdno_core::hash::content_hash;

    // Simulate the state right after the FTS migration: notes are already
    // indexed (their content hash matches) but have no FTS rows yet. The
    // per-file pass will skip them on hash, so the dedicated FTS-heal pass
    // is the only thing that can fill search — exactly what it's for.
    let (store, index) = fixtures();
    let raw = "---\ntype: inbox\ntitle: A title\n---\n# Body\n\nsearchable backfill text\n";
    store.write_file(&vp("inbox/a.md"), raw).unwrap();

    // Pre-seed the note row with the *correct* hash (so phase 1 skips it)
    // and deliberately no FTS row.
    index
        .upsert_note(&NoteEntry {
            path: vp("inbox/a.md"),
            note_type: "inbox".to_owned(),
            title: Some("A title".to_owned()),
            content_hash: content_hash(raw),
            mtime_ns: 1,
            size: raw.len() as u64,
            frontmatter: json!({}),
            indexed_at_ns: 1,
        })
        .unwrap();
    assert!(index.search("searchable", 10).unwrap().is_empty());

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    // Phase 1 skipped the unchanged note; phase 3 backfilled its FTS row.
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.fts_built, 1);
    let hits = index.search("searchable", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, vp("inbox/a.md"));
}

#[test]
fn reconcile_drops_orphan_fts_rows() {
    // An FTS row whose note no longer exists on disk or in the index is
    // dropped by the heal pass.
    let (store, index) = fixtures();
    index
        .replace_fts(&vp("gone.md"), Some("Gone"), "orphan body")
        .unwrap();
    assert_eq!(index.fts_indexed_paths().unwrap(), vec![vp("gone.md")]);

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    assert_eq!(report.fts_removed, 1);
    assert!(index.fts_indexed_paths().unwrap().is_empty());
    assert!(report.errors.is_empty());
}

/// Test wrapper that delegates every `VaultIndex` method to an inner
/// `MemoryIndex` except `remove_note`, which always errors. Kept
/// inline in this file because it's specific to the orphan-failure
/// test; the broader `FailingIndex` in `transaction_tests.rs` is not
/// accessible from this integration test binary.
struct FailOnRemoveIndex {
    inner: Arc<MemoryIndex>,
}

impl VaultIndex for FailOnRemoveIndex {
    fn upsert_note(&self, entry: &NoteEntry) -> Result<(), IndexError> {
        self.inner.upsert_note(entry)
    }
    fn remove_note(&self, _path: &VaultPath) -> Result<(), IndexError> {
        Err(IndexError::Update("forced test failure".to_owned()))
    }
    fn find_by_path(&self, path: &VaultPath) -> Result<Option<NoteEntry>, IndexError> {
        self.inner.find_by_path(path)
    }
    fn list_by_type(&self, note_type: &str) -> Result<Vec<NoteEntry>, IndexError> {
        self.inner.list_by_type(note_type)
    }
    fn list_all_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.list_all_paths()
    }
    fn replace_deadlines(
        &self,
        path: &VaultPath,
        deadlines: &[DeadlineEntry],
    ) -> Result<(), IndexError> {
        self.inner.replace_deadlines(path, deadlines)
    }
    fn deadlines_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, DeadlineEntry)>, IndexError> {
        self.inner.deadlines_between(from, to)
    }
    fn replace_links(&self, path: &VaultPath, links: &[LinkEntry]) -> Result<(), IndexError> {
        self.inner.replace_links(path, links)
    }
    fn find_backlinks(&self, path: &VaultPath) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.find_backlinks(path)
    }
    fn find_outgoing_links(&self, path: &VaultPath) -> Result<Vec<LinkEntry>, IndexError> {
        self.inner.find_outgoing_links(path)
    }
    fn replace_tags(&self, path: &VaultPath, tags: &[String]) -> Result<(), IndexError> {
        self.inner.replace_tags(path, tags)
    }
    fn find_by_tag(&self, tag: &str) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.find_by_tag(tag)
    }
    fn replace_milestones(
        &self,
        path: &VaultPath,
        milestones: &[MilestoneEntry],
    ) -> Result<(), IndexError> {
        self.inner.replace_milestones(path, milestones)
    }
    fn milestones_for_project(&self, slug: &str) -> Result<Vec<MilestoneEntry>, IndexError> {
        self.inner.milestones_for_project(slug)
    }
    fn milestones_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, MilestoneEntry)>, IndexError> {
        self.inner.milestones_between(from, to)
    }
    fn record_archival_snapshot(
        &self,
        path: &VaultPath,
        snapshot: &cdno_core::index::ArchivalSnapshot,
    ) -> Result<(), IndexError> {
        self.inner.record_archival_snapshot(path, snapshot)
    }
    fn find_archival_snapshot(
        &self,
        path: &VaultPath,
    ) -> Result<Option<cdno_core::index::ArchivalSnapshot>, IndexError> {
        self.inner.find_archival_snapshot(path)
    }
    fn replace_fts(
        &self,
        path: &VaultPath,
        title: Option<&str>,
        body: &str,
    ) -> Result<(), IndexError> {
        self.inner.replace_fts(path, title, body)
    }
    fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<cdno_core::index::SearchHit>, IndexError> {
        self.inner.search(query, limit)
    }
    fn fts_indexed_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.fts_indexed_paths()
    }
}

// ---- #94: mtime fast-path -------------------------------------------------

/// Wraps a `MemoryVaultStore` and counts `read_file` calls, so a test can
/// prove the reconcile fast-path skips reading unchanged files. Every other
/// method delegates straight through.
struct CountingStore {
    inner: Arc<MemoryVaultStore>,
    reads: AtomicUsize,
}

impl CountingStore {
    fn new(inner: Arc<MemoryVaultStore>) -> Self {
        Self {
            inner,
            reads: AtomicUsize::new(0),
        }
    }
    fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }
    fn reset(&self) {
        self.reads.store(0, Ordering::Relaxed);
    }
}

impl VaultStore for CountingStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        self.inner.delete_file(path)
    }
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        self.inner.exists(path)
    }
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.list_dir(path)
    }
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.walk_dir(path)
    }
    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError> {
        self.inner.metadata(path)
    }
    fn import_external(&self, src: &std::path::Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.import_external(src, dest)
    }
}

#[test]
fn second_pass_skips_reading_unchanged_files() {
    let mem = Arc::new(MemoryVaultStore::new());
    seed_note(&mem, "inbox/a.md", "inbox", "");
    seed_note(&mem, "projects/b.md", "project", "status: active\n");

    let counting = Arc::new(CountingStore::new(mem));
    let store: Arc<dyn VaultStore> = counting.clone();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    // First pass indexes both files — each is read once.
    let first = reconcile(&store, &index, &IgnoreSet::empty()).unwrap();
    assert_eq!(first.added, 2);
    assert_eq!(counting.reads(), 2);

    // Second pass: nothing changed, so the mtime+size fast path skips both
    // without a single read.
    counting.reset();
    let second = reconcile(&store, &index, &IgnoreSet::empty()).unwrap();
    assert_eq!(second.added, 0);
    assert_eq!(second.updated, 0);
    assert_eq!(
        counting.reads(),
        0,
        "unchanged files must not be re-read on the second pass"
    );
}

#[test]
fn a_changed_file_is_still_detected_after_the_fast_path() {
    let mem = Arc::new(MemoryVaultStore::new());
    seed_note(&mem, "inbox/a.md", "inbox", "");
    seed_note(&mem, "inbox/b.md", "inbox", "");

    let counting = Arc::new(CountingStore::new(mem.clone()));
    let store: Arc<dyn VaultStore> = counting.clone();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    reconcile(&store, &index, &IgnoreSet::empty()).unwrap();

    // Edit one file — write_file bumps its mtime, so the fast path misses it
    // (and only it) on the next pass.
    mem.write_file(
        &vp("inbox/a.md"),
        "---\ntype: inbox\n---\n# A\n\nchanged body\n",
    )
    .unwrap();

    counting.reset();
    let report = reconcile(&store, &index, &IgnoreSet::empty()).unwrap();
    assert_eq!(report.updated, 1);
    assert_eq!(
        counting.reads(),
        1,
        "only the changed file is read; the untouched one fast-paths"
    );
}

#[test]
fn a_touched_but_unchanged_file_is_restamped_then_fast_paths() {
    // mtime bumped, bytes identical (a `touch`, a git checkout, a
    // save-without-edit). The first pass after the touch still has to read
    // to confirm via the hash, but it must re-stamp the row so the *next*
    // pass fast-paths instead of re-reading forever.
    let mem = Arc::new(MemoryVaultStore::new());
    let raw = "---\ntype: inbox\ntitle: A\n---\n# A\n\nstable body\n".to_owned();
    mem.write_file(&vp("inbox/a.md"), &raw).unwrap();

    let counting = Arc::new(CountingStore::new(mem.clone()));
    let store: Arc<dyn VaultStore> = counting.clone();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    reconcile(&store, &index, &IgnoreSet::empty()).unwrap();

    // Rewrite identical bytes — bumps the memory store's mtime, same content.
    mem.write_file(&vp("inbox/a.md"), &raw).unwrap();

    // Pass after the touch: fast path misses (mtime drifted), so the file is
    // read once and hash-matched; nothing is reindexed.
    counting.reset();
    let report = reconcile(&store, &index, &IgnoreSet::empty()).unwrap();
    assert_eq!(report.updated, 0);
    assert_eq!(report.added, 0);
    assert_eq!(counting.reads(), 1, "touched file is read once to confirm");

    // Next pass: the re-stamp made mtime match, so it fast-paths — no read.
    counting.reset();
    reconcile(&store, &index, &IgnoreSet::empty()).unwrap();
    assert_eq!(
        counting.reads(),
        0,
        "a touched-but-identical file must not re-read forever"
    );
}

#[test]
fn commit_stamps_the_real_file_mtime_so_a_written_note_fast_paths() {
    // A domain-style write: the entry carries a placeholder mtime (the domain
    // uses the entry-build instant, never the file's mtime). The commit seam
    // must correct it to the file's real mtime, so the very next reconcile
    // fast-paths the note instead of re-reading it (#94, approach B).
    let mem = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    let path = vp("inbox/a.md");
    let content = "---\ntype: inbox\n---\n# A\n\nbody\n";
    let entry = NoteEntry {
        path: path.clone(),
        note_type: "inbox".to_owned(),
        title: None,
        content_hash: content_hash(content),
        mtime_ns: 1, // deliberately wrong; commit should overwrite it
        size: 1,
        frontmatter: json!({}),
        indexed_at_ns: 1,
    };

    let store_dyn: Arc<dyn VaultStore> = mem.clone();
    let mut tx = VaultTransaction::new(store_dyn, index.clone()).expect("write lock");
    tx.write_file(path.clone(), content);
    tx.upsert_note(entry);
    tx.commit().unwrap();

    // The stored row now matches the file on disk, not the placeholder.
    let stored = index.find_by_path(&path).unwrap().unwrap();
    let meta = mem.metadata(&path).unwrap();
    assert_eq!(
        stored.mtime_ns,
        meta.mtime_ns(),
        "mtime corrected at commit"
    );
    assert_eq!(stored.size, meta.size, "size corrected at commit");

    // And so reconcile reads nothing — the note fast-paths immediately.
    let counting = Arc::new(CountingStore::new(mem));
    let store: Arc<dyn VaultStore> = counting.clone();
    reconcile(&store, &index, &IgnoreSet::empty()).unwrap();
    assert_eq!(
        counting.reads(),
        0,
        "a freshly-written note should fast-path on the first reconcile"
    );
}

#[test]
fn ignore_glob_skips_matching_file_entirely() {
    let (store, index) = fixtures();
    seed_note(&store, "projects/cuaderno.md", "project", "");
    // A repo doc that lives in the vault dir but isn't a note: no
    // frontmatter, so reconciling it would otherwise record an error.
    store
        .write_file(
            &vp("CLAUDE.md"),
            "# CLAUDE\n\nrepo instructions, not a note\n",
        )
        .unwrap();

    let ignore = IgnoreSet::compile(&["CLAUDE.md".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();

    // The ignored file never enters the pass: not scanned, no error, no
    // index row. Only the real note is reflected, and the exclusion is
    // counted so it isn't silent.
    assert_eq!(report.scanned, 1);
    assert_eq!(report.ignored, 1);
    assert!(report.errors.is_empty());
    assert!(
        index
            .find_by_path(&vp("projects/cuaderno.md"))
            .unwrap()
            .is_some()
    );
    assert!(index.find_by_path(&vp("CLAUDE.md")).unwrap().is_none());
}

#[test]
fn without_ignore_a_non_note_file_reconciles_with_an_error() {
    // Counterpart to the test above: the same stray file, left
    // un-ignored, is walked and fails to parse — proving the ignore
    // glob is what suppresses it, not some other filter.
    let (store, index) = fixtures();
    store
        .write_file(
            &vp("CLAUDE.md"),
            "# CLAUDE\n\nrepo instructions, not a note\n",
        )
        .unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.errors.len(), 1);
    assert!(index.find_by_path(&vp("CLAUDE.md")).unwrap().is_none());
}

#[test]
fn newly_ignored_file_is_removed_from_index_as_orphan() {
    let (store, index) = fixtures();
    seed_note(&store, "README.md", "zettel", "");
    seed_note(&store, "projects/cuaderno.md", "project", "");

    // First pass: nothing ignored, both notes indexed.
    reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert!(index.find_by_path(&vp("README.md")).unwrap().is_some());

    // Add README.md to the ignore list and reconcile again: it falls
    // out of the filesystem set, so Phase 2 drops the now-stale row.
    let ignore = IgnoreSet::compile(&["README.md".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();
    assert_eq!(report.removed, 1);
    assert!(index.find_by_path(&vp("README.md")).unwrap().is_none());
    // Phase 2's remove_note cascades to the FTS row too, so the ignored
    // note also leaves the search index — the guarantee that backs
    // "absent from search". (The cascade happens in Phase 2, not the
    // Phase-3 FTS-heal, so report.fts_removed stays 0 here.)
    assert!(
        !index
            .fts_indexed_paths()
            .unwrap()
            .contains(&vp("README.md"))
    );
    assert!(
        index
            .find_by_path(&vp("projects/cuaderno.md"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn ignore_glob_matches_nested_paths_with_doublestar() {
    let (store, index) = fixtures();
    seed_note(&store, "projects/cuaderno.md", "project", "");
    seed_note(&store, "inbox/scratch.draft.md", "zettel", "");

    let ignore = IgnoreSet::compile(&["**/*.draft.md".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();
    assert_eq!(report.scanned, 1);
    assert!(report.errors.is_empty());
    assert!(
        index
            .find_by_path(&vp("inbox/scratch.draft.md"))
            .unwrap()
            .is_none()
    );
    assert!(
        index
            .find_by_path(&vp("projects/cuaderno.md"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn ignore_literal_is_root_anchored_and_does_not_cross_segments() {
    // A bare-name pattern matches only at the vault root, and (with
    // literal_separator) `*` does not cross `/`. This pins the
    // gitignore-ish semantics the docs promise: `*.md` must NOT swallow
    // nested notes, and `CLAUDE.md` must NOT match `projects/CLAUDE.md`.
    let (store, index) = fixtures();
    seed_note(&store, "CLAUDE-notes.md", "zettel", "");
    seed_note(&store, "projects/CLAUDE-notes.md", "project", "");
    seed_note(&store, "inbox/scratch.md", "zettel", "");

    // `CLAUDE-notes.md` (root-anchored) + `*.md` (single segment only).
    let ignore = IgnoreSet::compile(&["CLAUDE-notes.md".to_string(), "*.md".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();

    // Both root-level files dropped; both nested ones survive — `*.md`
    // and the bare name stayed at the root.
    assert!(
        index
            .find_by_path(&vp("CLAUDE-notes.md"))
            .unwrap()
            .is_none()
    );
    assert!(
        index
            .find_by_path(&vp("projects/CLAUDE-notes.md"))
            .unwrap()
            .is_some()
    );
    assert!(
        index
            .find_by_path(&vp("inbox/scratch.md"))
            .unwrap()
            .is_some()
    );
    assert_eq!(report.scanned, 2);
}

#[test]
fn ignore_applies_every_pattern_in_the_set() {
    // The whole point of a GlobSet is multiple patterns; confirm each
    // one is live, not just the first.
    let (store, index) = fixtures();
    seed_note(&store, "CLAUDE.md", "zettel", "");
    seed_note(&store, "README.md", "zettel", "");
    seed_note(&store, "projects/cuaderno.md", "project", "");

    let ignore = IgnoreSet::compile(&["CLAUDE.md".to_string(), "README.md".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.ignored, 2);
    assert!(index.find_by_path(&vp("CLAUDE.md")).unwrap().is_none());
    assert!(index.find_by_path(&vp("README.md")).unwrap().is_none());
    assert!(
        index
            .find_by_path(&vp("projects/cuaderno.md"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn ignore_pattern_matching_nothing_leaves_the_vault_intact() {
    // A pattern that matches no file must not perturb the index — guards
    // against an over-broad matcher silently dropping notes.
    let (store, index) = fixtures();
    seed_note(&store, "projects/cuaderno.md", "project", "");
    seed_note(&store, "journal/daily/2026-04-19.md", "daily", "");

    let ignore = IgnoreSet::compile(&["nonexistent/**".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();
    assert_eq!(report.scanned, 2);
    assert_eq!(report.added, 2);
    assert_eq!(report.ignored, 0);
    assert!(report.errors.is_empty());
}

#[test]
fn over_broad_ignore_empties_index_but_files_survive_and_recover() {
    // The prime directive: even a catastrophic `**` must never lose a
    // note. It empties the *index* (via Phase-2 orphan removal) but the
    // files on disk are untouched, and removing the glob restores every
    // row. This is the test that guarantees "we never nuke the notes".
    let (store, index) = fixtures();
    seed_note(&store, "projects/cuaderno.md", "project", "");
    seed_note(&store, "journal/daily/2026-04-19.md", "daily", "");
    seed_note(&store, "inbox/scratch.md", "zettel", "");

    // Baseline: everything indexed.
    let base = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(base.added, 3);

    // `**` matches every path: the index is wiped, the exclusion counted.
    let nuke = IgnoreSet::compile(&["**".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &nuke).unwrap();
    assert_eq!(report.ignored, 3);
    assert_eq!(report.removed, 3);
    assert!(index.list_all_paths().unwrap().is_empty());

    // ...but the markdown files are all still on disk.
    assert_eq!(store.walk_dir(&VaultPath::root()).unwrap().len(), 3);

    // Recovery: drop the glob and reconcile — every note re-enters the
    // index from the surviving files. Fully recoverable, zero data loss.
    let restored = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();
    assert_eq!(restored.added, 3);
    assert_eq!(restored.ignored, 0);
    assert!(
        index
            .find_by_path(&vp("projects/cuaderno.md"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn ignore_negation_is_literal_not_re_inclusion() {
    // globset has no gitignore `!` re-inclusion. `["*.md", "!keep.md"]`
    // must NOT rescue keep.md: `*.md` still excludes it (root segment),
    // and `!keep.md` is just a literal pattern for a file so named.
    let (store, index) = fixtures();
    seed_note(&store, "keep.md", "zettel", "");
    seed_note(&store, "projects/cuaderno.md", "project", "");

    let ignore = IgnoreSet::compile(&["*.md".to_string(), "!keep.md".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();
    assert!(index.find_by_path(&vp("keep.md")).unwrap().is_none());
    assert!(
        index
            .find_by_path(&vp("projects/cuaderno.md"))
            .unwrap()
            .is_some()
    );
    assert_eq!(report.ignored, 1);
}

#[test]
fn root_anchored_and_empty_patterns_exclude_nothing() {
    // A leading `/` does not anchor (paths are already root-relative) and
    // an empty pattern matches no path — both are inert, never match-all.
    let (store, index) = fixtures();
    seed_note(&store, "CLAUDE.md", "zettel", "");
    seed_note(&store, "projects/cuaderno.md", "project", "");

    let ignore = IgnoreSet::compile(&["/CLAUDE.md".to_string(), String::new()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();
    assert_eq!(report.ignored, 0);
    assert_eq!(report.scanned, 2);
}

#[test]
fn invalid_ignore_glob_is_a_config_error() {
    // An unclosed character class is malformed; it surfaces as a
    // ConfigError at compile time rather than silently matching nothing.
    let err = IgnoreSet::compile(&["a[".to_string()]).unwrap_err();
    assert!(matches!(err, cdno_core::error::ConfigError::InvalidGlob(_)));
}

// ---------------------------------------------------------------------
// Attachment artefacts (#451). Filing a document into a portfolio writes
// an evidence stub beside a folder holding the artefact. A markdown
// artefact has no frontmatter, so indexing it can only ever fail — it is
// excluded by location, not by extension.
// ---------------------------------------------------------------------

const STUB: &str = "---\ntype: evidence\ncreated: 2026-07-03\nsource: A filed document\nportfolio: demo\norigin: \"[[projects/foo]]\"\nkind: file\n---\n# A filed document\n\n## Abstract\n\nWhat it says.\n";

/// A filed markdown artefact: a plain document, no frontmatter — exactly
/// what reconciliation used to choke on.
const ARTEFACT_MD: &str = "# Reviewer notes\n\nVerdict: approve with changes.\n";

#[test]
fn markdown_artefact_is_not_indexed_and_is_counted() {
    let (store, index) = fixtures();
    store
        .write_file(&vp("portfolios/demo/2026-07-03-panel.md"), STUB)
        .unwrap();
    store
        .write_file(
            &vp("portfolios/demo/2026-07-03-panel/02-reviewer-b.md"),
            ARTEFACT_MD,
        )
        .unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    // The stub is a note; its artefact is not. No error either way — the
    // artefact never enters the pass, so it cannot fail to parse.
    assert_eq!(report.scanned, 1);
    assert_eq!(report.artefacts, 1);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
    assert!(
        index
            .find_by_path(&vp("portfolios/demo/2026-07-03-panel.md"))
            .unwrap()
            .is_some()
    );
    assert!(
        index
            .find_by_path(&vp("portfolios/demo/2026-07-03-panel/02-reviewer-b.md"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn markdown_in_a_folder_without_a_stub_is_still_a_note() {
    // Only a stub-owned folder is exempt. An unpaired subfolder holds
    // notes, and a note that fails to parse must still be reported —
    // otherwise the exemption becomes a silent catch-all.
    let (store, index) = fixtures();
    store
        .write_file(&vp("portfolios/demo/loose/notes.md"), ARTEFACT_MD)
        .unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    assert_eq!(report.artefacts, 0);
    assert_eq!(report.scanned, 1);
    assert_eq!(report.errors.len(), 1);
}

#[test]
fn ignoring_a_stub_does_not_promote_its_artefacts_to_notes() {
    // Ownership is resolved against the full markdown set, before the
    // `ignore` partition, so the two exclusions stay independent.
    let (store, index) = fixtures();
    store
        .write_file(&vp("portfolios/demo/2026-07-03-panel.md"), STUB)
        .unwrap();
    store
        .write_file(
            &vp("portfolios/demo/2026-07-03-panel/02-reviewer-b.md"),
            ARTEFACT_MD,
        )
        .unwrap();

    let ignore = IgnoreSet::compile(&["portfolios/demo/2026-07-03-panel.md".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();

    assert_eq!(report.artefacts, 1);
    assert_eq!(report.ignored, 1);
    assert_eq!(report.scanned, 0);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
}

#[test]
fn artefact_exemption_reaches_through_a_grouping_folder() {
    // Depth-independence (#454): a stub nested under a grouping folder
    // owns its artefacts exactly as one at the portfolio root does.
    let (store, index) = fixtures();
    store
        .write_file(&vp("portfolios/demo/sweep/2026-07-03-run-07.md"), STUB)
        .unwrap();
    store
        .write_file(
            &vp("portfolios/demo/sweep/2026-07-03-run-07/log.md"),
            ARTEFACT_MD,
        )
        .unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index), &IgnoreSet::empty()).unwrap();

    assert_eq!(report.artefacts, 1);
    assert_eq!(report.scanned, 1);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
}

// ---------------------------------------------------------------------
// Folder-scoped ignore globs (#440). `**` is recursive and `*` stays
// within one segment, so `folder/*/**` matches one *or more* components
// after the folder — the semantics that evicted every portfolio note
// from a real vault when the intent was to exclude only what sits two
// levels down.
// ---------------------------------------------------------------------

#[test]
fn folder_scoped_glob_with_one_star_matches_at_every_depth_below() {
    let (store, index) = fixtures();
    seed_note(&store, "portfolios/demo/_index.md", "portfolio", "");
    seed_note(&store, "portfolios/demo/2026-07-03-note.md", "evidence", "");
    seed_note(&store, "portfolios/demo/group/nested.md", "evidence", "");
    seed_note(&store, "projects/alpha.md", "project", "");

    let ignore = IgnoreSet::compile(&["portfolios/*/**".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();

    // Everything under a portfolio folder goes, at one level and at two.
    assert_eq!(report.ignored, 3);
    assert_eq!(report.scanned, 1);
    assert!(
        index
            .find_by_path(&vp("projects/alpha.md"))
            .unwrap()
            .is_some()
    );
    assert!(
        index
            .find_by_path(&vp("portfolios/demo/_index.md"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn folder_scoped_glob_with_two_stars_spares_the_level_above() {
    // The pattern the vault actually wanted: exclude artefacts two levels
    // below `portfolios/`, leaving the index and evidence notes indexed.
    let (store, index) = fixtures();
    seed_note(&store, "portfolios/demo/_index.md", "portfolio", "");
    seed_note(&store, "portfolios/demo/2026-07-03-note.md", "evidence", "");
    seed_note(&store, "portfolios/demo/group/nested.md", "evidence", "");

    let ignore = IgnoreSet::compile(&["portfolios/*/*/**".to_string()]).unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index), &ignore).unwrap();

    assert_eq!(report.ignored, 1);
    assert_eq!(report.scanned, 2);
    assert!(
        index
            .find_by_path(&vp("portfolios/demo/_index.md"))
            .unwrap()
            .is_some()
    );
}
