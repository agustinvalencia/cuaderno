//! `WriteJournal` echo-window behaviour.

use cdno_core::path::VaultPath;
use cdno_tauri::state::WriteJournal;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

#[test]
fn recorded_paths_are_recent_self_writes() {
    let journal = WriteJournal::default();
    journal.record([
        vp("projects/alpha.md"),
        vp("journal/2026/daily/2026-07-07.md"),
    ]);

    assert!(journal.is_recent_self_write(&vp("projects/alpha.md")));
    assert!(journal.is_recent_self_write(&vp("journal/2026/daily/2026-07-07.md")));
    assert!(!journal.is_recent_self_write(&vp("projects/beta.md")));
}

#[test]
fn unrecorded_journal_matches_nothing() {
    let journal = WriteJournal::default();
    assert!(!journal.is_recent_self_write(&vp("projects/alpha.md")));
}
