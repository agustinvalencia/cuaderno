//! Filesystem watching: the `FileWatcher` trait and its `notify`-backed
//! implementation.
//!
//! Live consumers (the desktop app's watcher thread) subscribe to
//! debounced batches of vault-relative change events and react by
//! re-running [`crate::reconcile::reconcile`] — events are *hints*
//! about where to look, never the source of truth. That posture is
//! deliberate: editors save atomically (write a temp file, rename it
//! over the target), platform backends coalesce and occasionally drop
//! events, and the debouncer merges bursts — so consumers must stay
//! correct on imprecise input. Reconcile is path-agnostic and cheap
//! (mtime fast path), which is what makes that contract workable.
//!
//! The trait deviates from the original sketch in
//! `docs/implementation-plan.md` §3.4 (four created/modified/deleted/
//! moved variants): after debouncing, the backends can't reliably
//! distinguish create from modify, and a rename surfaces as two
//! paths. What a debounced watcher *can* say honestly is "this path
//! exists now" versus "this path is gone" — plus "something was
//! missed, rescan". Those are the three variants.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer, notify};

use crate::path::VaultPath;

/// How long the debouncer waits for a path to go quiet before
/// emitting it. 400ms absorbs editor atomic-save storms (write +
/// rename + metadata touches) into one batch without making the UI
/// feel laggy.
pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(400);

/// One debounced filesystem observation, vault-relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEvent {
    /// The path exists after the debounce window — created or
    /// modified (indistinguishable post-debounce), or the destination
    /// of a rename.
    Changed(VaultPath),
    /// The path no longer exists — deleted, or the source of a rename.
    Removed(VaultPath),
    /// The backend reported an **error**: events may have been
    /// dropped, so the consumer should treat the whole vault as
    /// potentially changed (full reconcile + global cache
    /// invalidation) rather than trusting the batch to be complete.
    ///
    /// Known limitation: inotify *queue overflow* does NOT reach this
    /// variant — the mini debouncer forwards overflow as a path-less
    /// `Ok` event and drops it, so overflow is indistinguishable from
    /// silence here. Consumers must not rely on `Rescan` as their
    /// only staleness backstop (the desktop app pairs it with
    /// focus-refetch and startup reconciliation for exactly this
    /// reason).
    Rescan,
}

/// Watching failed to start or attach to the vault root.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("failed to start filesystem watcher: {0}")]
    Init(String),
    #[error("failed to watch {path}: {reason}")]
    Watch { path: String, reason: String },
}

/// Watch a vault for filesystem changes and deliver debounced,
/// vault-relative event batches.
pub trait FileWatcher: Send {
    /// Start watching. Batches are sent to `sender` until [`stop`]
    /// (or drop) — a consumer that goes away simply drops its
    /// receiver, and subsequent sends are discarded.
    ///
    /// [`stop`]: FileWatcher::stop
    fn watch(&mut self, sender: Sender<Vec<FileEvent>>) -> Result<(), WatchError>;

    /// Stop watching. Idempotent; a stopped watcher can be
    /// re-started with a fresh `watch` call.
    fn stop(&mut self);
}

/// `notify`-backed [`FileWatcher`] over a vault root directory
/// (FSEvents on macOS, inotify on Linux), debounced by
/// [`DEBOUNCE_WINDOW`].
///
/// Emits every path under the root relativised into [`VaultPath`] —
/// including non-markdown files and `.cuaderno/` internals. Filtering
/// is deliberately the *consumer's* policy (the desktop watcher
/// thread drops non-`.md`, `.cuaderno/`, and config-ignored paths):
/// this type only knows about the filesystem, not about what counts
/// as a note. Paths that escape the root (shouldn't happen) or fail
/// `VaultPath`'s invariants are dropped rather than panicking the
/// callback.
pub struct FsFileWatcher {
    root: PathBuf,
    debouncer: Option<Debouncer<notify::RecommendedWatcher>>,
}

impl FsFileWatcher {
    /// A watcher for the vault at `root`. Watching starts on
    /// [`FileWatcher::watch`], not here.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            debouncer: None,
        }
    }
}

impl FileWatcher for FsFileWatcher {
    fn watch(&mut self, sender: Sender<Vec<FileEvent>>) -> Result<(), WatchError> {
        // Canonicalise ONCE, and use the same root for both the watch
        // registration and the relativisation. The two must agree:
        // FSEvents (macOS) reports resolved paths regardless of what
        // was registered, but inotify (Linux) reconstructs paths from
        // the *registered* root — so watching the raw root while
        // stripping the canonical one would silently drop every event
        // under a symlinked vault root.
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let strip_root = root.clone();
        let mut debouncer = new_debouncer(DEBOUNCE_WINDOW, move |result: DebounceEventResult| {
            let batch = map_debounce_result(&strip_root, result);
            if !batch.is_empty() {
                // A closed receiver just means the consumer is gone;
                // nothing useful to do with the error here.
                let _ = sender.send(batch);
            }
        })
        .map_err(|e| WatchError::Init(e.to_string()))?;

        debouncer
            .watcher()
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| WatchError::Watch {
                path: root.display().to_string(),
                reason: e.to_string(),
            })?;

        self.debouncer = Some(debouncer);
        Ok(())
    }

    fn stop(&mut self) {
        // Dropping the debouncer stops its worker thread and the
        // underlying platform watcher.
        self.debouncer = None;
    }
}

/// Map one debounced callback result to the batch the consumer sees.
/// Factored out of the debouncer closure so the error arm and the
/// path mapping are unit-testable without provoking a real backend
/// failure. A backend error becomes a lone [`FileEvent::Rescan`] (see
/// the variant's overflow caveat); dropped/unrepresentable paths
/// vanish rather than aborting the batch.
pub fn map_debounce_result(root: &Path, result: DebounceEventResult) -> Vec<FileEvent> {
    match result {
        Ok(events) => events
            .iter()
            .filter_map(|event| to_file_event(root, &event.path))
            .collect(),
        Err(_) => vec![FileEvent::Rescan],
    }
}

/// Map one debounced absolute path to a [`FileEvent`], or `None` for
/// paths outside the root / not representable as a [`VaultPath`].
/// Existence is probed *after* the debounce window, which is what
/// lets a rename collapse into `Removed(old)` + `Changed(new)`.
fn to_file_event(root: &Path, absolute: &Path) -> Option<FileEvent> {
    let relative = absolute.strip_prefix(root).ok()?;
    let vault_path = VaultPath::new(relative.to_str()?).ok()?;
    if absolute.exists() {
        Some(FileEvent::Changed(vault_path))
    } else {
        Some(FileEvent::Removed(vault_path))
    }
}
