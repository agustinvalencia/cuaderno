use std::sync::Arc;

use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::reconcile::reconcile;
use cdno_core::store::{MemoryVaultStore, VaultStore};

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
