//! The rename against the real thing (RFC 0007 T21): `FsVaultStore` and
//! `SqliteIndex` in a temp dir. The memory implementations cannot show the
//! performance budget (§8) or the order SQLite sees the index ops in.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use cdno_core::config::VaultConfig;
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{FsVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::{Context, EnergyLevel};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use tempfile::tempdir;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn build_fs_vault(root: &Path) -> (Vault, Arc<dyn VaultIndex>) {
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> =
        Arc::new(SqliteIndex::open(root.join(".cuaderno/index.db")).expect("open index"));
    let (vault, _report) =
        Vault::new(store, Arc::clone(&index), VaultConfig::default()).expect("construct vault");
    (vault, index)
}

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 6, 13)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
}

fn put(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

const PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n\n## Milestones\n";

fn action(project: &str, n: usize, body: &str) -> String {
    format!(
        "---\ntype: action\nstatus: active\nproject: {project}\nenergy: deep\ncreated: 2026-05-01\n---\n\n# Step {n}\n\n{body}\n"
    )
}

fn commitment(project: &str, n: usize, body: &str) -> String {
    format!(
        "---\ntype: commitment\nstatus: active\ndue: 2027-06-01\ncreated: 2026-05-01\ncontext: work\nproject: \"{project}\"\nstewardship: null\n---\n\n# Deliver {n}\n\n{body}\n"
    )
}

fn daily(date: NaiveDate, body: &str) -> String {
    format!("---\ntype: daily\ndate: {date}\n---\n\n## Logs\n- **09:00**: {body}\n")
}

#[test]
#[ignore = "a timing budget: run by hand with --ignored --nocapture"]
fn rename_on_two_thousand_notes_stays_under_budget() {
    const NOTES: usize = 2000;
    const REFERRERS: usize = 300;

    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cuaderno")).unwrap();
    put(
        root,
        "projects/old.md",
        &PROJECT.replace("# Surrogate model", "# Old"),
    );
    put(
        root,
        "projects/emulator.md",
        &PROJECT.replace("# Surrogate model", "# Emulator"),
    );

    // 1 998 more notes in a mix of types; the first 300 refer to `old`:
    // 100 actions (field and link), 100 commitments (field and link), 100
    // daily logs (links), the rest to `emulator` or to nothing.
    let start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    for n in 0..(NOTES - 2) {
        let date = start + chrono::Days::new(n as u64);
        let refers = n < REFERRERS;
        match n % 3 {
            0 if refers => put(
                root,
                &format!("actions/step-{n}.md"),
                &action("old", n, "See [[old]] and [[projects/old|the model]]."),
            ),
            1 if refers => put(
                root,
                &format!("commitments/deliver-{n}.md"),
                &commitment("old", n, "For [[old]]."),
            ),
            2 if refers => put(
                root,
                &format!("journal/{}/daily/{date}.md", date.format("%Y")),
                &daily(date, "worked on [[old]]"),
            ),
            0 => put(
                root,
                &format!("actions/step-{n}.md"),
                &action("emulator", n, "See [[emulator]]."),
            ),
            1 => put(
                root,
                &format!("commitments/deliver-{n}.md"),
                &commitment("emulator", n, "For [[emulator]]."),
            ),
            _ => put(
                root,
                &format!("journal/{}/daily/{date}.md", date.format("%Y")),
                &daily(date, "worked on [[emulator]]"),
            ),
        }
    }

    let (vault, index) = build_fs_vault(root);
    assert_eq!(
        index.list_all_paths().unwrap().len(),
        NOTES,
        "every generated note is indexed"
    );

    let t = Instant::now();
    let plan = vault
        .rename_preview("old", "surrogate-model", None, at())
        .expect("preview");
    let preview = t.elapsed();
    assert!(
        plan.edits.len() > REFERRERS,
        "the target and its {REFERRERS} referrers are edited, got {}",
        plan.edits.len()
    );

    let t = Instant::now();
    let outcome = vault
        .rename_note("old", "surrogate-model", None, Some(&plan.hash), at())
        .expect("rename");
    let write = t.elapsed();
    assert!(!outcome.index_stale);

    println!(
        "rename bench, {NOTES} notes, {REFERRERS} referrers: preview {preview:?}, write {write:?}"
    );
    assert!(preview.as_secs_f64() < 1.0, "preview took {preview:?}");
    assert!(write.as_secs_f64() < 3.0, "write took {write:?}");
}

#[test]
fn rename_index_ops_land_in_order_on_sqlite() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cuaderno")).unwrap();
    let (vault, index) = build_fs_vault(root);

    vault
        .create_project(at(), "Old", Context::Work, None)
        .expect("create project");
    let note = vault
        .add_action_with_note(at(), "old", "Characterise", EnergyLevel::Deep)
        .expect("add action");
    let abs = root.join(note.to_string());
    let raw = std::fs::read_to_string(&abs).unwrap();
    std::fs::write(&abs, format!("{raw}\nCalibrated against [[old]].\n")).unwrap();
    let map = root.join("projects/old.md");
    let text = std::fs::read_to_string(&map).unwrap();
    std::fs::write(
        &map,
        format!("{text}\nThe baseline is in [[actions/characterise]].\n"),
    )
    .unwrap();
    // Reconciliation takes the hand edits in before the archival.
    drop(vault);
    let (vault, index2) = build_fs_vault(root);
    drop(index);
    let index = index2;
    vault
        .complete_action(at(), "old", "characterise")
        .expect("complete action");
    let archived = vp("actions/_done/2026/characterise.md");
    let referrer = vp("journal/2026/daily/2026-06-13.md");
    vault
        .log_to_daily_note(at(), "calibrating [[old]]")
        .expect("log");

    let before = index
        .find_archival_snapshot(&archived)
        .unwrap()
        .expect("archival stamps a snapshot");
    assert!(
        index
            .link_resolutions()
            .unwrap()
            .iter()
            .any(|r| { r.target_raw == "old" && r.resolved_path == Some(vp("projects/old.md")) }),
        "the links table resolves `old` before the rename"
    );

    let links_before = index
        .find_outgoing_links(&vp("projects/old.md"))
        .unwrap()
        .len();
    assert!(links_before > 0, "the project map links its action");

    vault
        .rename_note("old", "surrogate-model", None, None, at())
        .expect("rename");

    // The moved note's own facets are derived from its paired write; a pure
    // move followed by an upsert would leave the new path without them.
    assert_eq!(
        index
            .find_outgoing_links(&vp("projects/surrogate-model.md"))
            .unwrap()
            .len(),
        links_before,
        "the new path carries the note's links"
    );
    assert!(
        index
            .find_outgoing_links(&vp("projects/old.md"))
            .unwrap()
            .is_empty()
    );

    assert!(
        index
            .find_by_path(&vp("projects/old.md"))
            .unwrap()
            .is_none()
    );
    assert!(
        index
            .find_by_path(&vp("projects/surrogate-model.md"))
            .unwrap()
            .is_some()
    );
    assert!(index.find_by_path(&referrer).unwrap().is_some());
    let after = index
        .find_archival_snapshot(&archived)
        .unwrap()
        .expect("the snapshot row survives");
    assert_eq!(after, before, "the archived referrer's snapshot row");
    let stale: Vec<_> = index
        .link_resolutions()
        .unwrap()
        .into_iter()
        .filter(|r| r.resolved_path == Some(vp("projects/old.md")))
        .collect();
    assert!(
        stale.is_empty(),
        "links still resolved to the old path: {stale:?}"
    );
    assert!(
        index
            .link_resolutions()
            .unwrap()
            .iter()
            .any(|r| r.resolved_path == Some(vp("projects/surrogate-model.md"))),
        "links resolve to the new path"
    );
    // The files agree.
    assert!(!root.join("projects/old.md").exists());
    assert!(root.join("projects/surrogate-model.md").exists());
}
