//! The batch decision table (`plan_batch`) — pure, no AppHandle.

use std::time::{Duration, Instant};

use cdno_core::path::VaultPath;
use cdno_core::watcher::FileEvent;
use cdno_tauri::events::VaultArea;
use cdno_tauri::state::{ECHO_WINDOW, WriteJournal};
use cdno_tauri::watcher::{BatchPlan, plan_batch};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

#[test]
fn external_edits_classify_sort_and_dedup() {
    let journal = WriteJournal::default();
    let plan = plan_batch(
        &journal,
        vec![
            FileEvent::Changed(vp("stewardships/health.md")),
            FileEvent::Changed(vp("projects/alpha.md")),
            FileEvent::Removed(vp("projects/beta.md")),
        ],
    );
    assert_eq!(
        plan,
        BatchPlan::External {
            areas: vec![VaultArea::Projects, VaultArea::Stewardships],
            paths: vec![
                "stewardships/health.md".into(),
                "projects/alpha.md".into(),
                "projects/beta.md".into(),
            ],
        }
    );
}

#[test]
fn rescan_wins_over_everything_in_the_batch() {
    let journal = WriteJournal::default();
    let plan = plan_batch(
        &journal,
        vec![
            FileEvent::Changed(vp("projects/alpha.md")),
            FileEvent::Rescan,
        ],
    );
    assert_eq!(plan, BatchPlan::Rescan);
}

#[test]
fn self_echoes_and_noise_are_quiet() {
    let journal = WriteJournal::default();
    journal.record([vp("projects/alpha.md")]);
    let plan = plan_batch(
        &journal,
        vec![
            // Our own write echoing back.
            FileEvent::Changed(vp("projects/alpha.md")),
            // Non-note noise: attachments, index db, temp staging.
            FileEvent::Changed(vp("assets/photo.png")),
            FileEvent::Changed(vp(".cuaderno/index.db")),
        ],
    );
    assert_eq!(plan, BatchPlan::Quiet);
}

#[test]
fn mixed_batch_emits_only_the_external_subset() {
    let journal = WriteJournal::default();
    journal.record([vp("journal/2026/daily/2026-07-07.md")]);
    let plan = plan_batch(
        &journal,
        vec![
            FileEvent::Changed(vp("journal/2026/daily/2026-07-07.md")),
            FileEvent::Changed(vp("projects/alpha.md")),
        ],
    );
    assert_eq!(
        plan,
        BatchPlan::External {
            areas: vec![VaultArea::Projects],
            paths: vec!["projects/alpha.md".into()],
        }
    );
}

#[test]
fn journal_entries_expire_after_the_echo_window() {
    let journal = WriteJournal::default();
    let wrote_at = Instant::now();
    journal.record_at(wrote_at, [vp("projects/alpha.md")]);

    let just_inside = wrote_at + ECHO_WINDOW - Duration::from_millis(1);
    assert!(journal.is_recent_self_write_at(just_inside, &vp("projects/alpha.md")));

    let just_past = wrote_at + ECHO_WINDOW;
    assert!(
        !journal.is_recent_self_write_at(just_past, &vp("projects/alpha.md")),
        "an echo past the window must be treated as external (safer failure direction)"
    );
}
