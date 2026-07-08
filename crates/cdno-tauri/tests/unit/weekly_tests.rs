//! `get_weekly_bundle_impl` against the Memory doubles — the command
//! seam, no Tauri runtime involved. Exercises the composition the
//! Weekly Review depends on: existing weekly-note content winning over
//! the seed, the completed-actions wins source, and the stuck-project
//! scan carrying its day count.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::vault::WeeklySection;
use cdno_tauri::commands::weekly::get_weekly_bundle_impl;
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nUnderway.\n\n## Next Actions\n- [ ] Draft methods (deep)\n";

// A completed action note whose `completed:` date lands inside the
// reviewed week (Mon 2026-07-06 .. Sun 2026-07-12).
const DONE_ACTION: &str = "---\ntype: action\nstatus: completed\nproject: alpha\nenergy: deep\nmilestone: null\ndue: null\ncreated: 2026-07-01\ncompleted: 2026-07-08\nblocker: null\ncriteria: |\n  Reader wired.\n---\n\n# Wire the reader\n";

#[test]
fn bundle_composes_existing_wins_completed_actions_and_stuck_project() {
    let vault = vault_with(&[
        ("projects/alpha.md", ALPHA),
        ("actions/wire-reader.md", DONE_ACTION),
    ]);

    // Seed the week's note with real Wins content so the "prefer
    // existing over seed" rule has something to prefer. Wednesday of
    // the reviewed week; the impl normalises to the Monday anchor.
    let anchor = ymd(2026, 7, 8);
    vault
        .upsert_weekly_section(anchor, WeeklySection::Wins, "We shipped M6.", false)
        .expect("seed wins");

    // A zero-day stuck threshold: the Memory store stamps mtime at
    // construction, so only "modified today or earlier" catches a
    // just-written project — which is exactly what makes ALPHA register
    // as stuck here. `today` must be the REAL current date for the
    // stuck scan (its day count is measured against the wall-clock
    // mtime), while `anchor` stays fixed to pin the completed-actions
    // window deterministically.
    let today = chrono::Local::now().date_naive();
    let bundle = get_weekly_bundle_impl(&vault, today, anchor, 0).unwrap();

    // Anchor normalises to the Monday of the ISO week.
    assert_eq!(bundle.week_of, ymd(2026, 7, 6));

    // Existing note content is parsed and carried (beats the seed).
    assert_eq!(bundle.weekly.wins.as_deref(), Some("We shipped M6."));
    assert!(bundle.weekly.exists);
    assert!(bundle.weekly.challenges.is_none());

    // The completed action inside the week is a wins source.
    assert_eq!(bundle.completed_actions.len(), 1);
    assert_eq!(bundle.completed_actions[0].title, "Wire the reader");
    assert_eq!(bundle.completed_actions[0].project, "alpha");

    // The active project shows in the scan and, at a zero-day
    // threshold, in the stuck set with its day count.
    assert!(bundle.projects.iter().any(|p| p.slug == "alpha"));
    let alpha_stuck = bundle
        .stuck
        .iter()
        .find(|s| s.slug == "alpha")
        .expect("alpha is stuck at a zero-day threshold");
    assert_eq!(alpha_stuck.days_unchanged, 0);
}

#[test]
fn bundle_without_a_weekly_note_reports_absent_sections() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);
    let anchor = ymd(2026, 7, 8);

    let today = chrono::Local::now().date_naive();
    let bundle = get_weekly_bundle_impl(&vault, today, anchor, 0).unwrap();

    // No note yet: exists is false and every section is None, so the
    // frontend seeds fresh rather than preferring an empty section.
    assert!(!bundle.weekly.exists);
    assert!(bundle.weekly.wins.is_none());
    assert!(bundle.weekly.this_weeks_goal.is_none());
    assert!(bundle.completed_actions.is_empty());
    // The following week has no note either, so next week's goal is None
    // and the Focus step starts blank rather than echoing this week's.
    assert!(bundle.next_week_goal.is_none());
    // The focus save targets the Monday AFTER the reviewed week's Monday.
    assert_eq!(bundle.week_of, ymd(2026, 7, 6));
    assert_eq!(bundle.next_week_of, ymd(2026, 7, 13));
}

#[test]
fn focus_save_targets_next_week_and_leaves_the_reviewed_week_untouched() {
    // The Focus step's "next week's focus" must land in NEXT week's note,
    // never overwrite the goal of the week being reviewed. This exercises
    // the seam the frontend uses: read the bundle, then save the goal to
    // `bundle.next_week_of`.
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);
    let anchor = ymd(2026, 7, 8); // Wednesday of the reviewed week.

    // The reviewed week already carries a goal set by planning; the
    // review must leave it alone.
    vault
        .upsert_weekly_section(
            anchor,
            WeeklySection::ThisWeeksGoal,
            "Ship M6 this week.",
            false,
        )
        .expect("seed the reviewed week's goal");

    let today = chrono::Local::now().date_naive();
    let bundle = get_weekly_bundle_impl(&vault, today, anchor, 0).unwrap();
    assert_eq!(bundle.week_of, ymd(2026, 7, 6));
    assert_eq!(bundle.next_week_of, ymd(2026, 7, 13));

    // The Focus step saves to the bundle's next_week_of — exactly what
    // the frontend echoes back — writing the goal into the FOLLOWING
    // week's note.
    vault
        .upsert_weekly_section(
            bundle.next_week_of,
            WeeklySection::ThisWeeksGoal,
            "Start M7.",
            false,
        )
        .expect("save next week's focus");

    // Path assertion: the write landed in the week-of-2026-07-13 note,
    // not the reviewed week's note.
    let next_week_note = vp(&cdno_core::paths::weekly_note_relpath(ymd(2026, 7, 13)));
    let reviewed_note = vp(&cdno_core::paths::weekly_note_relpath(ymd(2026, 7, 6)));
    assert_ne!(next_week_note, reviewed_note, "the two notes are distinct");

    let next_content = vault
        .read_weekly_note(ymd(2026, 7, 13))
        .expect("read next week's note");
    assert_eq!(next_content.path, next_week_note);
    assert!(
        next_content.markdown.contains("Start M7."),
        "next week's note carries the focus: {}",
        next_content.markdown
    );

    // The reviewed week's own goal is untouched by the focus save.
    let reviewed = get_weekly_bundle_impl(&vault, today, anchor, 0).unwrap();
    assert_eq!(
        reviewed.weekly.this_weeks_goal.as_deref(),
        Some("Ship M6 this week."),
        "the reviewed week's goal is not overwritten"
    );
    // And next week's goal now shows through the bundle for editing.
    assert_eq!(reviewed.next_week_goal.as_deref(), Some("Start M7."));
}
