use cdno_core::index::{
    DeadlineEntry, LinkEntry, MilestoneEntry, NoteEntry, SqliteIndex, VaultIndex,
};
use cdno_core::path::VaultPath;
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
    let (_d, idx) = store();
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
    let (_d, idx) = store();
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
    let (_d, idx) = store();
    assert!(idx.find_by_path(&vp("missing.md")).unwrap().is_none());
}

#[test]
fn list_by_type_returns_sorted_and_filtered() {
    let (_d, idx) = store();
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
    let (_d, idx) = store();
    let n = sample_note("projects/foo.md", "project");
    idx.upsert_note(&n).unwrap();
    idx.remove_note(&n.path).unwrap();

    assert!(idx.find_by_path(&n.path).unwrap().is_none());
}

#[test]
fn remove_note_cascades_all_facets() {
    let (_d, idx) = store();
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
        }],
    )
    .unwrap();
    idx.replace_tags(&n.path, &["deep-work".to_owned()])
        .unwrap();
    idx.replace_milestones(
        &n.path,
        &[MilestoneEntry {
            name: "ship v1".to_owned(),
            date: Some("2026-05-01".to_owned()),
            is_hard: true,
            completed: false,
        }],
    )
    .unwrap();

    idx.remove_note(&n.path).unwrap();

    assert!(
        idx.deadlines_between("2026-01-01", "2027-01-01")
            .unwrap()
            .is_empty()
    );
    assert!(idx.find_outgoing_links(&n.path).unwrap().is_empty());
    assert!(idx.find_by_tag("deep-work").unwrap().is_empty());
    assert!(
        idx.milestones_between("2026-01-01", "2027-01-01")
            .unwrap()
            .is_empty()
    );
    assert!(idx.milestones_for_project("foo").unwrap().is_empty());
}

#[test]
fn replace_deadlines_overwrites_prior_set() {
    let (_d, idx) = store();
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
    let (_d, idx) = store();
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
fn replace_links_and_traverse_graph() {
    let (_d, idx) = store();
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
            },
            LinkEntry {
                target_raw: "c".to_owned(),
                resolved_path: Some(vp("projects/c.md")),
                label: Some("see also".to_owned()),
            },
            LinkEntry {
                target_raw: "ghost".to_owned(),
                resolved_path: None,
                label: None,
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
        }],
    )
    .unwrap();

    let outgoing = idx.find_outgoing_links(&vp("projects/a.md")).unwrap();
    assert_eq!(outgoing.len(), 3);
    assert_eq!(outgoing[2].target_raw, "ghost");
    assert!(outgoing[2].resolved_path.is_none());

    let c_backlinks = idx.find_backlinks(&vp("projects/c.md")).unwrap();
    assert_eq!(c_backlinks, vec![vp("projects/a.md"), vp("projects/b.md")]);

    let b_backlinks = idx.find_backlinks(&vp("projects/b.md")).unwrap();
    assert_eq!(b_backlinks, vec![vp("projects/a.md")]);

    // ghost links (resolved_path IS NULL) never show up in backlinks.
    let ghost_backlinks = idx.find_backlinks(&vp("projects/ghost.md")).unwrap();
    assert!(ghost_backlinks.is_empty());
}

#[test]
fn replace_tags_overwrites_prior_set() {
    let (_d, idx) = store();
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
    let (_d, idx) = store();
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
fn vault_index_is_dyn_compatible_and_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SqliteIndex>();
    let (_d, idx) = store();
    let _boxed: Box<dyn VaultIndex> = Box::new(idx);
}

#[test]
fn list_all_paths_is_empty_on_fresh_index() {
    let (_d, idx) = store();
    assert!(idx.list_all_paths().unwrap().is_empty());
}

#[test]
fn list_all_paths_returns_every_inserted_path_sorted() {
    let (_d, idx) = store();
    idx.upsert_note(&sample_note("projects/b.md", "project"))
        .unwrap();
    idx.upsert_note(&sample_note("projects/a.md", "project"))
        .unwrap();
    idx.upsert_note(&sample_note("journal/daily/2026-04-19.md", "daily"))
        .unwrap();

    let paths = idx.list_all_paths().unwrap();
    assert_eq!(
        paths,
        vec![
            vp("journal/daily/2026-04-19.md"),
            vp("projects/a.md"),
            vp("projects/b.md"),
        ]
    );
}

#[test]
fn list_all_paths_drops_rows_after_remove_note() {
    let (_d, idx) = store();
    idx.upsert_note(&sample_note("a.md", "daily")).unwrap();
    idx.upsert_note(&sample_note("b.md", "daily")).unwrap();
    idx.remove_note(&vp("a.md")).unwrap();
    assert_eq!(idx.list_all_paths().unwrap(), vec![vp("b.md")]);
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
    let (_d, idx) = store();
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

    let got = idx.milestones_for_project("foo").unwrap();
    let names: Vec<&str> = got.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["kickoff", "submission", "someday"]);
    assert!(got[1].is_hard && !got[1].completed);
    assert_eq!(got[2].date, None);
}

#[test]
fn milestones_for_project_resolves_parked_location() {
    let (_d, idx) = store();
    idx.upsert_note(&sample_note("projects/_parked/dormant.md", "project"))
        .unwrap();
    idx.replace_milestones(
        &vp("projects/_parked/dormant.md"),
        &[milestone("revisit", Some("2026-09-01"), false, false)],
    )
    .unwrap();

    let got = idx.milestones_for_project("dormant").unwrap();
    assert_eq!(
        got,
        vec![milestone("revisit", Some("2026-09-01"), false, false)]
    );
}

#[test]
fn replace_milestones_overwrites_prior_set() {
    let (_d, idx) = store();
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

    let got = idx.milestones_for_project("foo").unwrap();
    assert_eq!(
        got,
        vec![milestone("second", Some("2026-06-01"), true, false)]
    );
}

#[test]
fn milestones_between_filters_by_date_and_sorts_across_projects() {
    let (_d, idx) = store();
    idx.upsert_note(&sample_note("projects/a.md", "project"))
        .unwrap();
    idx.upsert_note(&sample_note("projects/b.md", "project"))
        .unwrap();
    idx.replace_milestones(
        &vp("projects/a.md"),
        &[
            milestone("a-late", Some("2026-08-01"), true, false),
            // Undated and out-of-window entries must be excluded.
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
    let pairs: Vec<(&str, &str)> = got
        .iter()
        .map(|(p, m)| (p.as_path().to_str().unwrap(), m.name.as_str()))
        .collect();
    assert_eq!(
        pairs,
        vec![("projects/b.md", "b-mid"), ("projects/a.md", "a-late")],
    );
}
