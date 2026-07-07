//! Integration tests for `FsFileWatcher` against a real temporary
//! directory — the one place real platform watchers (FSEvents /
//! inotify) get exercised. Timeouts are generous because backend
//! delivery latency varies wildly across platforms and CI load; if
//! these ever flake in CI, mark them `#[ignore]` and rely on local
//! runs (the consumer's correctness never depends on event fidelity —
//! see the module docs).

use std::fs;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cdno_core::path::VaultPath;
use cdno_core::watcher::{FileEvent, FileWatcher, FsFileWatcher};

const RECV_TIMEOUT: Duration = Duration::from_secs(10);

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Drain batches until `pred` matches one event or the timeout
/// elapses. Batching is nondeterministic (one edit can arrive as one
/// or several batches), so tests assert on the union.
fn wait_for(
    rx: &mpsc::Receiver<Vec<FileEvent>>,
    pred: impl Fn(&FileEvent) -> bool,
) -> Option<FileEvent> {
    let deadline = Instant::now() + RECV_TIMEOUT;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let Ok(batch) = rx.recv_timeout(remaining) else {
            break;
        };
        if let Some(event) = batch.into_iter().find(&pred) {
            return Some(event);
        }
    }
    None
}

#[test]
fn watcher_reports_created_file_as_changed() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("projects")).unwrap();

    let (tx, rx) = mpsc::channel();
    let mut watcher = FsFileWatcher::new(dir.path());
    watcher.watch(tx).unwrap();

    fs::write(dir.path().join("projects/alpha.md"), "# Alpha\n").unwrap();

    let event = wait_for(
        &rx,
        |e| matches!(e, FileEvent::Changed(p) if *p == vp("projects/alpha.md")),
    );
    assert!(event.is_some(), "expected Changed(projects/alpha.md)");
    watcher.stop();
}

#[test]
fn watcher_reports_deleted_file_as_removed() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("note.md");
    fs::write(&target, "# Note\n").unwrap();

    let (tx, rx) = mpsc::channel();
    let mut watcher = FsFileWatcher::new(dir.path());
    watcher.watch(tx).unwrap();

    fs::remove_file(&target).unwrap();

    let event = wait_for(
        &rx,
        |e| matches!(e, FileEvent::Removed(p) if *p == vp("note.md")),
    );
    assert!(event.is_some(), "expected Removed(note.md)");
    watcher.stop();
}

#[test]
fn watcher_collapses_atomic_save_to_final_path() {
    // Editor-style atomic save: write a temp file, rename it over the
    // target. The debounced union must contain Changed(final path);
    // the temp path may surface as Removed — that's fine, consumers
    // filter by extension.
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("note.md");
    fs::write(&target, "v1\n").unwrap();

    let (tx, rx) = mpsc::channel();
    let mut watcher = FsFileWatcher::new(dir.path());
    watcher.watch(tx).unwrap();

    let tmp = dir.path().join(".note.md.tmp-1234");
    fs::write(&tmp, "v2\n").unwrap();
    fs::rename(&tmp, &target).unwrap();

    let event = wait_for(
        &rx,
        |e| matches!(e, FileEvent::Changed(p) if *p == vp("note.md")),
    );
    assert!(event.is_some(), "expected Changed(note.md) after rename");
    watcher.stop();
}

#[test]
fn stopped_watcher_stops_delivering() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut watcher = FsFileWatcher::new(dir.path());
    watcher.watch(tx).unwrap();
    watcher.stop();

    fs::write(dir.path().join("late.md"), "too late\n").unwrap();

    // Dropping the debouncer also drops the sender it captured, so
    // the channel reports disconnection rather than delivering.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut got_late_event = false;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(batch) => {
                got_late_event |= batch
                    .iter()
                    .any(|e| matches!(e, FileEvent::Changed(p) if *p == vp("late.md")));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        }
    }
    assert!(!got_late_event, "no events after stop()");
}
