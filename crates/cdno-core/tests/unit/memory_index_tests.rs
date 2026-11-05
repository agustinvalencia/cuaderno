//! Parallel suite for `MemoryIndex`. Mirrors the `SqliteIndex` cases in
//! `vault_index_tests.rs` to lock in identical behaviour across
//! production and test-fake implementations.

use cdno_core::index::{
    ArchivalSnapshot, DeadlineEntry, LinkEntry, LinkResolution, LinkResolutionUpdate, MemoryIndex,
    MilestoneEntry, NoteEntry, VaultIndex,
};
use cdno_core::path::VaultPath;
use serde_json::json;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn sample_note(path: &str, note_type: &str) -> NoteEntry {
    NoteEntry {
        path: vp(path),
        note_type: note_type.to_owned(),
        title: Some(format!("{path} title")),
        content_hash: "abc123".to_owned(),
        mtime_ns: 1_700_000_000_000_000_000,
        size: 128,
        frontmatter: json!({ "status": "active", "tags": ["alpha"] }),
        indexed_at_ns: 1_700_000_000_000_000_000,
    }
}

#[test]
fn upsert_note_then_find_by_path() {
    let idx = MemoryIndex::new();
    let n = sample_note("projects/foo.md", "project");
    idx.upsert_note(&n).unwrap();

    let fetched = idx.find_by_path(&vp("projects/foo.md")).unwrap().unwrap();
    assert_eq!(fetched.path, n.path);
    assert_eq!(fetched.note_type, "project");
    assert_eq!(fetched.title.as_deref(), Some("projects/foo.md title"));
    assert_eq!(fetched.frontmatter, n.frontmatter);
}

#[test]
fn upsert_note_updates_existing() {
    let idx = MemoryIndex::new();
    let mut n = sample_note("projects/foo.md", "project");
    idx.upsert_note(&n).unwrap();

    n.title = Some("updated".to_owned());
    n.content_hash = "def456".to_owned();
    idx.upsert_note(&n).unwrap();

    let fetched = idx.find_by_path(&vp("projects/foo.md")).unwrap().unwrap();
    assert_eq!(fetched.title.as_deref(), Some("updated"));
    assert_eq!(fetched.content_hash, "def456");
}

#[test]
fn find_by_path_missing_returns_none() {
    let idx = MemoryIndex::new();
    assert!(idx.find_by_path(&vp("missing.md")).unwrap().is_none());
}

#[test]
fn list_by_type_returns_sorted_and_filtered() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("projects/b.md", "project"))
        .unwrap();
    idx.upsert_note(&sample_note("projects/a.md", "project"))
        .unwrap();
    idx.upsert_note(&sample_note("journal/daily/2026-04-19.md", "daily"))
        .unwrap();

    let projects = idx.list_by_type("project").unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].path, vp("projects/a.md"));
    assert_eq!(projects[1].path, vp("projects/b.md"));

    let dailies = idx.list_by_type("daily").unwrap();
    assert_eq!(dailies.len(), 1);

    let unknown = idx.list_by_type("stewardship").unwrap();
    assert!(unknown.is_empty());
}

#[test]
fn remove_note_deletes_row() {
    let idx = MemoryIndex::new();
    let n = sample_note("projects/foo.md", "project");
    idx.upsert_note(&n).unwrap();
    idx.remove_note(&n.path).unwrap();

    assert!(idx.find_by_path(&n.path).unwrap().is_none());
}

#[test]
fn remove_note_cascades_all_facets() {
    let idx = MemoryIndex::new();
    let n = sample_note("projects/foo.md", "project");
    idx.upsert_note(&n).unwrap();

    idx.replace_deadlines(
        &n.path,
        &[DeadlineEntry {
            source: "project_milestone".to_owned(),
            title: "ship v1".to_owned(),
            due_date: "2026-05-01".to_owned(),
            is_hard: true,
            context: Some("work".to_owned()),
        }],
    )
    .unwrap();
    idx.replace_links(
        &n.path,
        &[LinkEntry {
            target_raw: "another".to_owned(),
            resolved_path: Some(vp("projects/another.md")),
            label: None,
            via_alias: false,
        }],
    )
    .unwrap();
    idx.replace_tags(&n.path, &["deep-work".to_owned()])
        .unwrap();

    idx.remove_note(&n.path).unwrap();

    assert!(
        idx.deadlines_between("2026-01-01", "2027-01-01")
            .unwrap()
            .is_empty()
    );
    assert!(idx.find_outgoing_links(&n.path).unwrap().is_empty());
    assert!(idx.find_by_tag("deep-work").unwrap().is_empty());
}

#[test]
fn replace_deadlines_overwrites_prior_set() {
    let idx = MemoryIndex::new();
    let n = sample_note("projects/foo.md", "project");
    idx.upsert_note(&n).unwrap();

    idx.replace_deadlines(
        &n.path,
        &[DeadlineEntry {
            source: "project_milestone".to_owned(),
            title: "first".to_owned(),
            due_date: "2026-05-01".to_owned(),
            is_hard: true,
            context: None,
        }],
    )
    .unwrap();

    idx.replace_deadlines(
        &n.path,
        &[DeadlineEntry {
            source: "project_milestone".to_owned(),
            title: "second".to_owned(),
            due_date: "2026-06-01".to_owned(),
            is_hard: false,
            context: Some("personal".to_owned()),
        }],
    )
    .unwrap();

    let all = idx.deadlines_between("2026-01-01", "2027-01-01").unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].1.title, "second");
    assert_eq!(all[0].1.due_date, "2026-06-01");
    assert!(!all[0].1.is_hard);
    assert_eq!(all[0].1.context.as_deref(), Some("personal"));
}

#[test]
fn deadlines_between_filters_by_range() {
    let idx = MemoryIndex::new();
    let n = sample_note("projects/foo.md", "project");
    idx.upsert_note(&n).unwrap();

    idx.replace_deadlines(
        &n.path,
        &[
            DeadlineEntry {
                source: "project_milestone".to_owned(),
                title: "early".to_owned(),
                due_date: "2026-01-15".to_owned(),
                is_hard: true,
                context: None,
            },
            DeadlineEntry {
                source: "project_milestone".to_owned(),
                title: "in-window".to_owned(),
                due_date: "2026-05-01".to_owned(),
                is_hard: true,
                context: None,
            },
            DeadlineEntry {
                source: "project_milestone".to_owned(),
                title: "late".to_owned(),
                due_date: "2026-12-01".to_owned(),
                is_hard: true,
                context: None,
            },
        ],
    )
    .unwrap();

    let window = idx.deadlines_between("2026-04-01", "2026-07-01").unwrap();
    assert_eq!(window.len(), 1);
    assert_eq!(window[0].1.title, "in-window");
}

#[test]
fn deadlines_between_sorted_by_due_date_across_notes() {
    // Unique to MemoryIndex: confirm the ordering contract holds when
    // deadlines come from more than one source note — SqliteIndex gets
    // this from `ORDER BY due_date`; memory has to sort explicitly.
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("a.md", "project")).unwrap();
    idx.upsert_note(&sample_note("b.md", "project")).unwrap();

    idx.replace_deadlines(
        &vp("a.md"),
        &[DeadlineEntry {
            source: "project_milestone".to_owned(),
            title: "a-late".to_owned(),
            due_date: "2026-06-15".to_owned(),
            is_hard: true,
            context: None,
        }],
    )
    .unwrap();
    idx.replace_deadlines(
        &vp("b.md"),
        &[DeadlineEntry {
            source: "project_milestone".to_owned(),
            title: "b-early".to_owned(),
            due_date: "2026-05-01".to_owned(),
            is_hard: true,
            context: None,
        }],
    )
    .unwrap();

    let all = idx.deadlines_between("2026-01-01", "2027-01-01").unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].1.title, "b-early");
    assert_eq!(all[1].1.title, "a-late");
}

#[test]
fn replace_links_and_traverse_graph() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("projects/a.md", "project"))
        .unwrap();
    idx.upsert_note(&sample_note("projects/b.md", "project"))
        .unwrap();
    idx.upsert_note(&sample_note("projects/c.md", "project"))
        .unwrap();

    idx.replace_links(
        &vp("projects/a.md"),
        &[
            LinkEntry {
                target_raw: "b".to_owned(),
                resolved_path: Some(vp("projects/b.md")),
                label: None,
                via_alias: false,
            },
            LinkEntry {
                target_raw: "c".to_owned(),
                resolved_path: Some(vp("projects/c.md")),
                label: Some("see also".to_owned()),
                via_alias: false,
            },
            LinkEntry {
                target_raw: "ghost".to_owned(),
                resolved_path: None,
                label: None,
                via_alias: false,
            },
        ],
    )
    .unwrap();
    idx.replace_links(
        &vp("projects/b.md"),
        &[LinkEntry {
            target_raw: "c".to_owned(),
            resolved_path: Some(vp("projects/c.md")),
            label: None,
            via_alias: false,
        }],
    )
    .unwrap();

    let outgoing = idx.find_outgoing_links(&vp("projects/a.md")).unwrap();
    assert_eq!(outgoing.len(), 3);
    // Insertion order preserved (same as SqliteIndex ORDER BY id).
    assert_eq!(outgoing[0].target_raw, "b");
    assert_eq!(outgoing[2].target_raw, "ghost");
    assert!(outgoing[2].resolved_path.is_none());

    let mut c_backlinks = idx.find_backlinks(&vp("projects/c.md")).unwrap();
    c_backlinks.sort_by(|a, b| a.as_path().cmp(b.as_path()));
    assert_eq!(c_backlinks, vec![vp("projects/a.md"), vp("projects/b.md")]);

    let b_backlinks = idx.find_backlinks(&vp("projects/b.md")).unwrap();
    assert_eq!(b_backlinks, vec![vp("projects/a.md")]);

    // Ghost links (resolved_path = None) never show up in backlinks.
    let ghost_backlinks = idx.find_backlinks(&vp("projects/ghost.md")).unwrap();
    assert!(ghost_backlinks.is_empty());
}

#[test]
fn find_backlinks_deduplicates_multiple_links_from_same_source() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("projects/a.md", "project"))
        .unwrap();
    idx.upsert_note(&sample_note("projects/z.md", "project"))
        .unwrap();

    // a.md links to z.md twice (e.g. once in the body, once in
    // frontmatter) — the backlink should still appear once.
    idx.replace_links(
        &vp("projects/a.md"),
        &[
            LinkEntry {
                target_raw: "z".to_owned(),
                resolved_path: Some(vp("projects/z.md")),
                label: None,
                via_alias: false,
            },
            LinkEntry {
                target_raw: "z again".to_owned(),
                resolved_path: Some(vp("projects/z.md")),
                label: Some("see also".to_owned()),
                via_alias: false,
            },
        ],
    )
    .unwrap();

    let backlinks = idx.find_backlinks(&vp("projects/z.md")).unwrap();
    assert_eq!(backlinks, vec![vp("projects/a.md")]);
}

#[test]
fn replace_tags_overwrites_prior_set() {
    let idx = MemoryIndex::new();
    let n = sample_note("journal/daily/2026-04-19.md", "daily");
    idx.upsert_note(&n).unwrap();

    idx.replace_tags(&n.path, &["alpha".to_owned(), "beta".to_owned()])
        .unwrap();
    idx.replace_tags(&n.path, &["gamma".to_owned()]).unwrap();

    assert!(idx.find_by_tag("alpha").unwrap().is_empty());
    assert!(idx.find_by_tag("beta").unwrap().is_empty());
    assert_eq!(idx.find_by_tag("gamma").unwrap(), vec![n.path]);
}

#[test]
fn find_by_tag_returns_all_matching_notes_sorted() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("b.md", "daily")).unwrap();
    idx.upsert_note(&sample_note("a.md", "daily")).unwrap();
    idx.upsert_note(&sample_note("c.md", "daily")).unwrap();

    idx.replace_tags(&vp("a.md"), &["collab".to_owned()])
        .unwrap();
    idx.replace_tags(&vp("c.md"), &["collab".to_owned()])
        .unwrap();
    idx.replace_tags(&vp("b.md"), &["other".to_owned()])
        .unwrap();

    let tagged = idx.find_by_tag("collab").unwrap();
    assert_eq!(tagged, vec![vp("a.md"), vp("c.md")]);
}

#[test]
fn replace_tags_dedupes_duplicates_in_input() {
    // SqliteIndex uses INSERT OR IGNORE against a composite PK to reject
    // duplicates; MemoryIndex should match that semantics so the suites
    // don't diverge when the same tag appears twice in a note.
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("x.md", "daily")).unwrap();
    idx.replace_tags(
        &vp("x.md"),
        &["dup".to_owned(), "dup".to_owned(), "other".to_owned()],
    )
    .unwrap();

    let tagged = idx.find_by_tag("dup").unwrap();
    assert_eq!(tagged, vec![vp("x.md")]);
    let other = idx.find_by_tag("other").unwrap();
    assert_eq!(other, vec![vp("x.md")]);
}

#[test]
fn memory_index_is_send_sync_and_dyn_compatible() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MemoryIndex>();
    let _boxed: Box<dyn VaultIndex> = Box::new(MemoryIndex::new());
}

fn milestone(name: &str, date: Option<&str>, is_hard: bool, completed: bool) -> MilestoneEntry {
    MilestoneEntry {
        name: name.to_owned(),
        date: date.map(str::to_owned),
        is_hard,
        completed,
    }
}

#[test]
fn milestones_for_project_returns_in_source_order() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("projects/foo.md", "project"))
        .unwrap();
    idx.replace_milestones(
        &vp("projects/foo.md"),
        &[
            milestone("kickoff", Some("2026-01-10"), false, true),
            milestone("submission", Some("2026-05-22"), true, false),
            milestone("someday", None, false, false),
        ],
    )
    .unwrap();

    let got = idx.milestones_for_project(&vp("projects/foo.md")).unwrap();
    let names: Vec<&str> = got.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["kickoff", "submission", "someday"]);
    assert_eq!(got[2].date, None);
}

#[test]
fn milestones_for_project_reads_the_given_path() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("projects/_parked/dormant.md", "project"))
        .unwrap();
    idx.replace_milestones(
        &vp("projects/_parked/dormant.md"),
        &[milestone("revisit", Some("2026-09-01"), false, false)],
    )
    .unwrap();

    // Passing the parked path returns the milestone.
    assert_eq!(
        idx.milestones_for_project(&vp("projects/_parked/dormant.md"))
            .unwrap(),
        vec![milestone("revisit", Some("2026-09-01"), false, false)],
    );

    // Passing the active path returns none (the milestone is only at parked).
    assert!(
        idx.milestones_for_project(&vp("projects/dormant.md"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn replace_milestones_overwrites_prior_set() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("projects/foo.md", "project"))
        .unwrap();
    idx.replace_milestones(
        &vp("projects/foo.md"),
        &[milestone("first", Some("2026-05-01"), true, false)],
    )
    .unwrap();
    idx.replace_milestones(
        &vp("projects/foo.md"),
        &[milestone("second", Some("2026-06-01"), true, false)],
    )
    .unwrap();

    assert_eq!(
        idx.milestones_for_project(&vp("projects/foo.md")).unwrap(),
        vec![milestone("second", Some("2026-06-01"), true, false)],
    );
}

#[test]
fn milestones_between_filters_by_date_and_sorts_across_projects() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("projects/a.md", "project"))
        .unwrap();
    idx.upsert_note(&sample_note("projects/b.md", "project"))
        .unwrap();
    idx.replace_milestones(
        &vp("projects/a.md"),
        &[
            milestone("a-late", Some("2026-08-01"), true, false),
            milestone("a-undated", None, false, false),
            milestone("a-early", Some("2025-01-01"), false, false),
        ],
    )
    .unwrap();
    idx.replace_milestones(
        &vp("projects/b.md"),
        &[milestone("b-mid", Some("2026-06-15"), false, false)],
    )
    .unwrap();

    let got = idx.milestones_between("2026-01-01", "2026-12-31").unwrap();
    let names: Vec<&str> = got.iter().map(|(_, m)| m.name.as_str()).collect();
    assert_eq!(names, vec!["b-mid", "a-late"]);
}

#[test]
fn remove_note_cascades_milestones() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("projects/foo.md", "project"))
        .unwrap();
    idx.replace_milestones(
        &vp("projects/foo.md"),
        &[milestone("ship", Some("2026-05-01"), true, false)],
    )
    .unwrap();

    idx.remove_note(&vp("projects/foo.md")).unwrap();
    assert!(
        idx.milestones_for_project(&vp("projects/foo.md"))
            .unwrap()
            .is_empty()
    );
    assert!(
        idx.milestones_between("2026-01-01", "2027-01-01")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn archival_snapshot_round_trips_and_cascades_on_note_delete() {
    let idx = MemoryIndex::new();
    let path = vp("actions/_done/2026/foo.md");
    idx.upsert_note(&sample_note("actions/_done/2026/foo.md", "action"))
        .unwrap();

    let snap = ArchivalSnapshot {
        frozen_size: 1234,
        frozen_hash: "deadbeefcafef00d".to_owned(),
        archived_at_ns: 1_700_000_000_000_000_000,
        masked_hash: Some("0123456789abcdef".to_owned()),
        mask_version: Some(1),
        masked_size: Some(1000),
    };
    idx.record_archival_snapshot(&path, &snap).unwrap();
    assert_eq!(idx.find_archival_snapshot(&path).unwrap(), Some(snap));

    idx.remove_note(&path).unwrap();
    assert!(idx.find_archival_snapshot(&path).unwrap().is_none());
}

#[test]
fn list_archival_snapshots_returns_every_row_sorted_by_path() {
    let idx = MemoryIndex::new();
    for p in ["actions/_done/2026/zeta.md", "actions/_done/2026/alpha.md"] {
        idx.upsert_note(&sample_note(p, "action")).unwrap();
    }
    let with_mask = ArchivalSnapshot {
        frozen_size: 10,
        frozen_hash: "deadbeefcafef00d".to_owned(),
        archived_at_ns: 1,
        masked_hash: Some("0123456789abcdef".to_owned()),
        mask_version: Some(1),
        masked_size: Some(8),
    };
    let without_mask = ArchivalSnapshot {
        masked_hash: None,
        mask_version: None,
        masked_size: None,
        ..with_mask.clone()
    };
    idx.record_archival_snapshot(&vp("actions/_done/2026/zeta.md"), &with_mask)
        .unwrap();
    idx.record_archival_snapshot(&vp("actions/_done/2026/alpha.md"), &without_mask)
        .unwrap();
    assert_eq!(
        idx.list_archival_snapshots().unwrap(),
        vec![
            (vp("actions/_done/2026/alpha.md"), without_mask),
            (vp("actions/_done/2026/zeta.md"), with_mask),
        ]
    );
}

// ---- full-text search (the lightweight double; not FTS5-faithful) -------

#[test]
fn search_finds_substring_in_title_or_body() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("projects/foo.md", "project"))
        .unwrap();
    idx.replace_fts(&vp("projects/foo.md"), Some("Budget plan"), "body text")
        .unwrap();

    // Title hit and body hit both resolve.
    assert_eq!(idx.search("budget", 10).unwrap().len(), 1);
    assert_eq!(idx.search("body", 10).unwrap().len(), 1);
    // Non-match returns nothing.
    assert!(idx.search("absent", 10).unwrap().is_empty());
}

#[test]
fn search_ranks_title_hits_before_body_hits() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("a.md", "inbox")).unwrap();
    idx.upsert_note(&sample_note("b.md", "inbox")).unwrap();
    idx.replace_fts(&vp("a.md"), Some("misc"), "the budget line")
        .unwrap();
    idx.replace_fts(&vp("b.md"), Some("Budget"), "unrelated")
        .unwrap();

    let hits = idx.search("budget", 10).unwrap();
    assert_eq!(hits.len(), 2);
    // b.md matched on title (score 0.0), a.md on body only (score 1.0).
    assert_eq!(hits[0].path, vp("b.md"));
    assert!(hits[0].score < hits[1].score);
}

#[test]
fn replace_fts_then_remove_note_clears_search() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("x.md", "inbox")).unwrap();
    idx.replace_fts(&vp("x.md"), None, "findable").unwrap();
    assert_eq!(idx.search("findable", 10).unwrap().len(), 1);
    assert_eq!(idx.fts_indexed_paths().unwrap(), vec![vp("x.md")]);

    idx.remove_note(&vp("x.md")).unwrap();
    assert!(idx.search("findable", 10).unwrap().is_empty());
    assert!(idx.fts_indexed_paths().unwrap().is_empty());
}

fn note_with(path: &str, frontmatter: serde_json::Value) -> NoteEntry {
    let mut n = sample_note(path, "concept");
    n.frontmatter = frontmatter;
    n
}

#[test]
fn aliases_are_read_from_the_frontmatter_json() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&note_with(
        "concepts/a.md",
        json!({ "aliases": ["old-slug", "A display name", 42] }),
    ))
    .unwrap();
    assert_eq!(
        idx.list_aliases().unwrap(),
        vec![("old-slug".to_owned(), vp("concepts/a.md"))]
    );
}

#[test]
fn a_scalar_alias_is_read() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&note_with(
        "concepts/a.md",
        json!({ "aliases": "old-slug" }),
    ))
    .unwrap();
    idx.upsert_note(&note_with(
        "concepts/b.md",
        json!({ "aliases": "Not a slug" }),
    ))
    .unwrap();
    assert_eq!(
        idx.list_aliases().unwrap(),
        vec![("old-slug".to_owned(), vp("concepts/a.md"))]
    );
}

#[test]
fn notes_without_aliases_contribute_nothing() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&sample_note("concepts/a.md", "concept"))
        .unwrap();
    idx.upsert_note(&note_with("concepts/b.md", json!({ "aliases": 7 })))
        .unwrap();
    idx.upsert_note(&note_with(
        "concepts/c.md",
        json!({ "aliases": { "k": "old-slug" } }),
    ))
    .unwrap();
    assert_eq!(idx.list_aliases().unwrap(), vec![]);
}

#[test]
fn aliases_are_ordered_by_path_then_position() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&note_with(
        "concepts/b.md",
        json!({ "aliases": ["b-two", "b-one"] }),
    ))
    .unwrap();
    idx.upsert_note(&note_with(
        "concepts/a.md",
        json!({ "aliases": ["a-two", "a-one"] }),
    ))
    .unwrap();
    let got: Vec<(String, String)> = idx
        .list_aliases()
        .unwrap()
        .into_iter()
        .map(|(a, p)| (a, p.to_string()))
        .collect();
    let want: Vec<(String, String)> = [
        ("a-two", "concepts/a.md"),
        ("a-one", "concepts/a.md"),
        ("b-two", "concepts/b.md"),
        ("b-one", "concepts/b.md"),
    ]
    .into_iter()
    .map(|(a, p)| (a.to_owned(), p.to_owned()))
    .collect();
    assert_eq!(got, want);
}

#[test]
fn quoted_numeric_and_bool_looking_aliases_are_read() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&note_with(
        "concepts/a.md",
        json!({ "aliases": ["2026", "no"] }),
    ))
    .unwrap();
    assert_eq!(
        idx.list_aliases().unwrap(),
        vec![
            ("2026".to_owned(), vp("concepts/a.md")),
            ("no".to_owned(), vp("concepts/a.md")),
        ]
    );
}

#[test]
fn an_alias_two_notes_declare_is_returned_twice() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&note_with(
        "concepts/a.md",
        json!({ "aliases": ["old-slug"] }),
    ))
    .unwrap();
    idx.upsert_note(&note_with(
        "concepts/b.md",
        json!({ "aliases": "old-slug" }),
    ))
    .unwrap();
    assert_eq!(
        idx.list_aliases().unwrap(),
        vec![
            ("old-slug".to_owned(), vp("concepts/a.md")),
            ("old-slug".to_owned(), vp("concepts/b.md")),
        ]
    );
}

#[test]
fn aliases_are_ordered_by_the_paths_text() {
    let idx = MemoryIndex::new();
    idx.upsert_note(&note_with(
        "x/foo/_index.md",
        json!({ "aliases": ["in-folder"] }),
    ))
    .unwrap();
    idx.upsert_note(&note_with("x/foo.md", json!({ "aliases": ["in-file"] })))
        .unwrap();
    // `.` sorts before `/`, so the flat note comes first.
    assert_eq!(
        idx.list_aliases().unwrap(),
        vec![
            ("in-file".to_owned(), vp("x/foo.md")),
            ("in-folder".to_owned(), vp("x/foo/_index.md")),
        ]
    );
}

#[test]
fn via_alias_round_trips_through_the_links_facet() {
    // RFC 0007 §5.5: `via_alias` is written by `replace_links` and by
    // `set_link_resolutions`, and read back by `find_outgoing_links` and
    // `link_resolutions`.
    let idx = MemoryIndex::new();
    for p in [
        "journal/d.md",
        "projects/surrogate-model.md",
        "notes/old.md",
    ] {
        idx.upsert_note(&sample_note(p, "project")).unwrap();
    }
    let src = vp("journal/d.md");
    let new = vp("projects/surrogate-model.md");
    idx.replace_links(
        &src,
        &[
            LinkEntry {
                target_raw: "old".to_owned(),
                resolved_path: Some(new.clone()),
                label: None,
                via_alias: true,
            },
            LinkEntry {
                target_raw: "surrogate-model".to_owned(),
                resolved_path: Some(new.clone()),
                label: None,
                via_alias: false,
            },
            LinkEntry {
                target_raw: "older".to_owned(),
                resolved_path: None,
                label: None,
                via_alias: false,
            },
        ],
    )
    .unwrap();

    let flags = |idx: &dyn VaultIndex| -> Vec<(String, Option<VaultPath>, bool)> {
        idx.find_outgoing_links(&src)
            .unwrap()
            .into_iter()
            .map(|l| (l.target_raw, l.resolved_path, l.via_alias))
            .collect()
    };
    assert_eq!(
        flags(&idx),
        vec![
            ("old".to_owned(), Some(new.clone()), true),
            ("surrogate-model".to_owned(), Some(new.clone()), false),
            ("older".to_owned(), None, false),
        ]
    );
    let mut resolutions = idx.link_resolutions().unwrap();
    resolutions.sort_by(|a, b| a.target_raw.cmp(&b.target_raw));
    assert_eq!(
        resolutions,
        vec![
            LinkResolution {
                target_raw: "old".to_owned(),
                resolved_path: Some(new.clone()),
                via_alias: true,
            },
            LinkResolution {
                target_raw: "older".to_owned(),
                resolved_path: None,
                via_alias: false,
            },
            LinkResolution {
                target_raw: "surrogate-model".to_owned(),
                resolved_path: Some(new.clone()),
                via_alias: false,
            },
        ]
    );

    // An update carries the flag both ways: `[[old]]` now finds a stem and
    // drops it, `[[older]]` resolves through an alias and gains it.
    let changed = idx
        .set_link_resolutions(&[
            LinkResolutionUpdate {
                target_raw: "old".to_owned(),
                from: Some(new.clone()),
                to: Some(vp("notes/old.md")),
                via_alias: false,
            },
            LinkResolutionUpdate {
                target_raw: "older".to_owned(),
                from: None,
                to: Some(new.clone()),
                via_alias: true,
            },
        ])
        .unwrap();
    assert_eq!(changed, 2);
    assert_eq!(
        flags(&idx),
        vec![
            ("old".to_owned(), Some(vp("notes/old.md")), false),
            ("surrogate-model".to_owned(), Some(new.clone()), false),
            ("older".to_owned(), Some(new.clone()), true),
        ]
    );
}
