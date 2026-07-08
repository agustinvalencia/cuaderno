//! `WriteJournal` echo-window behaviour, plus the `record_write` seam
//! that ties a domain [`WriteOutcome`] to the journal (#315): a real
//! write journals its full touched set (archival moves included), and a
//! no-op journals nothing.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::EnergyLevel;
use cdno_tauri::state::WriteJournal;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

/// A Memory-backed vault seeded with `notes`. Mirrors the domain unit
/// tests' fixture — no Tauri runtime, so the journal seam is exercised
/// directly against real domain writes.
fn vault_with(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

const ACTIVE_PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Foo\n\n## Current State\nGoing.\n\n## Next Actions\n";

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

#[test]
fn record_write_journals_the_archive_paths_after_a_complete_action() {
    // A completion whose bullet wikilinks an action note archives that
    // note in the same transaction. The desktop command feeds the
    // domain's WriteOutcome to `record_write`, which must journal EVERY
    // touched path — the project map, the daily, and both endpoints of
    // the archival move — so the watcher suppresses all of them (#315).
    let vault = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Characterise sample efficiency",
            EnergyLevel::Deep,
        )
        .expect("add succeeds");

    let outcome = vault
        .complete_action(dt(2026, 5, 27, 17, 0), "foo", "characterise")
        .expect("complete succeeds");

    let journal = WriteJournal::default();
    assert!(journal.record_write(&outcome), "a real write is recorded");

    for path in [
        "projects/foo.md",
        "journal/2026/daily/2026-05-27.md",
        "actions/characterise-sample-efficiency.md",
        "actions/_done/2026/characterise-sample-efficiency.md",
    ] {
        assert!(
            journal.is_recent_self_write(&vp(path)),
            "journal should suppress the self-write at {path}",
        );
    }
}

#[test]
fn record_write_leaves_the_journal_untouched_after_a_noop() {
    // An unchanged-state update is a silent domain no-op. Feeding its
    // WriteOutcome to `record_write` must record nothing and report
    // `false` (so the command also skips its emit) — otherwise it would
    // plant a false suppression entry that swallows a genuine external
    // edit to those paths for the echo window (#315).
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Foo\n\n## Current State\nSteady.\n\n## Next Actions\n";
    let vault = vault_with(&[("projects/foo.md", project)]);

    let outcome = vault
        .update_project_state(dt(2026, 5, 27, 17, 0), "foo", "Steady.")
        .expect("noop returns Ok");

    assert!(!outcome.touched(), "unchanged text is a no-op");

    let journal = WriteJournal::default();
    assert!(
        !journal.record_write(&outcome),
        "a no-op records nothing and signals the caller to skip its emit",
    );

    assert!(!journal.is_recent_self_write(&vp("projects/foo.md")));
    assert!(!journal.is_recent_self_write(&vp("journal/2026/daily/2026-05-27.md")));
}
