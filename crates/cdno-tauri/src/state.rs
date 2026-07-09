//! Tauri managed state: the shared `Vault` handle and the
//! self-write journal the watcher thread consults for echo
//! suppression.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;

use cdno_core::index::VaultIndex;
use cdno_core::path::VaultPath;
use cdno_core::store::VaultStore;
use cdno_domain::{Vault, WriteOutcome};

/// Everything the command layer needs, registered once via
/// `.manage(...)`.
///
/// `vault` is an [`ArcSwap`] rather than a bare `Arc` so a config edit
/// can be applied *live* (GH #365): a save reloads `.cuaderno/config.toml`,
/// rebuilds the `Vault` from the SAME store/index with the fresh config,
/// and atomically swaps the new handle in — no restart, no SQLite reopen.
/// An `ArcSwap` (not a `Mutex<Arc<Vault>>`) because the access pattern is
/// overwhelmingly read: every command loads the vault, a swap happens only
/// on the rare config reload. Reads never block, and — crucially —
/// [`AppState::vault`] hands each command an owned `Arc` snapshot, so a
/// command already running against the old vault finishes cleanly even as
/// a reload swaps a new one in underneath (correct by construction: the
/// loaded `Arc` keeps the old `Vault` alive until the closure returns).
/// `Vault`'s own methods take `&self` and writes stay serialised by the
/// cross-process lock at `VaultTransaction::new` (design plan §3.3), so no
/// wrapper lock is needed on top of the swap.
pub struct AppState {
    pub vault: ArcSwap<Vault>,
    /// The store and index the live vault was built on, retained so a
    /// config reload can rebuild the `Vault` (via `Vault::new`) WITHOUT
    /// reopening the SQLite index — the handle is reused across the swap.
    /// They are `Arc`, so holding a clone here is cheap; `Vault` itself
    /// deliberately does not re-expose them.
    pub store: Arc<dyn VaultStore>,
    pub index: Arc<dyn VaultIndex>,
    pub journal: WriteJournal,
    /// Absolute vault root, kept so `open_in_editor` can resolve a
    /// validated vault-relative path to a real file on disk, and so a
    /// config reload can re-read `.cuaderno/config.toml` from it. The
    /// domain works purely in `VaultPath`s and never needs the root,
    /// so this lives here rather than on `Vault`.
    pub root: std::path::PathBuf,
}

impl AppState {
    /// An owned snapshot of the currently-live vault.
    ///
    /// Returns [`ArcSwap::load_full`] — a full `Arc<Vault>` the caller (or
    /// the `spawn_blocking` closure it hands to `with_vault`) owns for the
    /// duration of its work. This is what makes the swap safe: a reload can
    /// store a new `Vault` at any moment, but a command holding this snapshot
    /// keeps running against the exact vault it loaded, never a half-swapped
    /// one.
    pub fn vault(&self) -> Arc<Vault> {
        self.vault.load_full()
    }
}

/// Paths this process wrote recently, so the watcher thread can tell
/// its own echoes from external edits (plan §3.2).
///
/// Write commands `record()` their touched paths after `commit()`;
/// the watcher skips the `vault:changed` emit for a batch whose every
/// path is inside the echo window (the command already emitted a
/// precise `origin: self` event). Correctness never depends on
/// suppression — reconcile still runs — so the failure modes are
/// benign in both directions: a missed suppression is one redundant
/// refetch, a wrong suppression is healed by the next event or focus
/// refetch.
#[derive(Default)]
pub struct WriteJournal {
    inner: Mutex<HashMap<VaultPath, Instant>>,
}

/// How long after a self-write the watcher treats an event on the
/// same path as our own echo. 2s covers the debounce window (400ms)
/// plus normal FSEvents delivery latency; under heavy coalescing a
/// late echo past the window costs one redundant refetch — the safer
/// failure direction versus suppressing a real external edit.
pub const ECHO_WINDOW: Duration = Duration::from_secs(2);

impl WriteJournal {
    /// Record paths this process just committed.
    pub fn record(&self, paths: impl IntoIterator<Item = VaultPath>) {
        self.record_at(Instant::now(), paths);
    }

    /// Journal a write [`WriteOutcome`]'s touched paths, returning
    /// whether anything was recorded.
    ///
    /// A no-op outcome (nothing written) records nothing and returns
    /// `false`: the caller must then also skip its `origin: self` emit,
    /// or it would announce — and, for the echo window, suppress genuine
    /// external edits to — paths this process never wrote (#315). This is
    /// the single seam that ties "did we write?" to "should we journal
    /// and emit?", so no command can get the pairing wrong.
    pub fn record_write(&self, outcome: &WriteOutcome) -> bool {
        if outcome.touched() {
            self.record(outcome.paths.iter().cloned());
            true
        } else {
            false
        }
    }

    /// Was `path` written by us within the echo window? Prunes stale
    /// entries as a side effect.
    pub fn is_recent_self_write(&self, path: &VaultPath) -> bool {
        self.is_recent_self_write_at(Instant::now(), path)
    }

    /// Clock-injected form of [`record`](Self::record) — exists so
    /// the expiry behaviour is testable without sleeping through a
    /// real window.
    #[doc(hidden)]
    pub fn record_at(&self, now: Instant, paths: impl IntoIterator<Item = VaultPath>) {
        let mut inner = self.inner.lock().expect("journal mutex poisoned");
        // Prune on every insert so the map never grows past the set
        // of paths written in the last window.
        inner.retain(|_, at| now.saturating_duration_since(*at) < ECHO_WINDOW);
        for path in paths {
            inner.insert(path, now);
        }
    }

    /// Clock-injected form of
    /// [`is_recent_self_write`](Self::is_recent_self_write).
    #[doc(hidden)]
    pub fn is_recent_self_write_at(&self, now: Instant, path: &VaultPath) -> bool {
        let mut inner = self.inner.lock().expect("journal mutex poisoned");
        inner.retain(|_, at| now.saturating_duration_since(*at) < ECHO_WINDOW);
        inner.contains_key(path)
    }
}
