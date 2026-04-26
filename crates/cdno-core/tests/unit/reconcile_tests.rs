use std::sync::Arc;

use cdno_core::error::IndexError;
use cdno_core::index::{DeadlineEntry, LinkEntry, MemoryIndex, NoteEntry, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::reconcile::reconcile;
use cdno_core::store::{MemoryVaultStore, VaultStore};
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
    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();
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

    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();
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
fn cuaderno_meta_files_are_excluded_from_scan() {
    let (store, index) = fixtures();
    // A real note at vault root.
    seed_note(&store, "journal/daily/2026-04-19.md", "daily", "");
    // A markdown file under the meta directory — must be invisible
    // to the indexer even though its extension and frontmatter type
    // would otherwise match a real note.
    seed_note(&store, ".cuaderno/templates/daily.md", "daily", "");

    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();

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

    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();

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
    reconcile(&as_store(&store), &as_index(&index)).unwrap();

    // Second run sees the same state — no adds/updates.
    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.removed, 0);
}

#[test]
fn changed_content_triggers_update() {
    let (store, index) = fixtures();
    seed_note(&store, "note.md", "daily", "");
    reconcile(&as_store(&store), &as_index(&index)).unwrap();

    // Rewrite with different content → hash changes.
    store
        .write_file(
            &vp("note.md"),
            "---\ntype: daily\ntitle: updated\n---\n# Body\nnew content\n",
        )
        .unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();
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
    reconcile(&as_store(&store), &as_index(&index)).unwrap();

    // Simulate external deletion of delete-me.md.
    store.delete_file(&vp("delete-me.md")).unwrap();

    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();
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

    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();
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

    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();
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

    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();
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

    reconcile(&as_store(&store), &as_index(&index)).unwrap();

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

    reconcile(&as_store(&store), &as_index(&index)).unwrap();

    let deadlines = index.deadlines_between("2026-01-01", "2027-01-01").unwrap();
    assert!(deadlines.is_empty(), "daily notes must not spawn deadlines");
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

    reconcile(&as_store(&store), &as_index(&index)).unwrap();

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
    reconcile(&as_store(&store), &as_index(&index)).unwrap();

    // Replace the milestone list.
    store
        .write_file(
            &vp("projects/p.md"),
            "---\ntype: project\n---\n# Milestones\n- [ ] second — hard: 2026-06-01\n",
        )
        .unwrap();
    let report = reconcile(&as_store(&store), &as_index(&index)).unwrap();
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
    reconcile(&as_store(&store), &as_index(&index)).unwrap();

    store.delete_file(&vp("projects/p.md")).unwrap();
    reconcile(&as_store(&store), &as_index(&index)).unwrap();

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
    reconcile(&as_store(&store), &as_index(&backing_index)).unwrap();

    // Simulate the file being deleted on disk.
    store.delete_file(&vp("orphan.md")).unwrap();

    // Run reconciliation through a wrapper whose remove_note always
    // fails. The underlying index is still the same backing store.
    let failing: Arc<dyn VaultIndex> = Arc::new(FailOnRemoveIndex {
        inner: backing_index.clone(),
    });
    let store_arc: Arc<dyn VaultStore> = store.clone();
    let report = reconcile(&store_arc, &failing).unwrap();

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
}
