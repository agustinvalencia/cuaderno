//! Tauri managed state: the shared `Vault` handle and the
//! self-write journal the watcher thread consults for echo
//! suppression.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cdno_core::path::VaultPath;
use cdno_domain::Vault;

/// Everything the command layer needs, registered once via
/// `.manage(...)`. `Vault` is shared as a bare `Arc` — no wrapper
/// lock — because its methods take `&self` and writes are already
/// serialised by the cross-process write lock acquired at
/// `VaultTransaction::new` (design plan §3.3; same posture as
/// `cdno-mcp`'s `CuadernoServer`).
pub struct AppState {
    pub vault: std::sync::Arc<Vault>,
    pub journal: WriteJournal,
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
        let now = Instant::now();
        let mut inner = self.inner.lock().expect("journal mutex poisoned");
        // Prune on every insert so the map never grows past the set
        // of paths written in the last window.
        inner.retain(|_, at| now.duration_since(*at) < ECHO_WINDOW);
        for path in paths {
            inner.insert(path, now);
        }
    }

    /// Was `path` written by us within the echo window? Prunes stale
    /// entries as a side effect.
    pub fn is_recent_self_write(&self, path: &VaultPath) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.lock().expect("journal mutex poisoned");
        inner.retain(|_, at| now.duration_since(*at) < ECHO_WINDOW);
        inner.contains_key(path)
    }
}
