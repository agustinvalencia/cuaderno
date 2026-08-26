//! The post-write sync-nudge sentinel (GH #540).
//!
//! A deployment that pairs `cdno-mcp-server` with an external sync
//! agent — a commit/push loop on an always-on host, say — otherwise
//! leaves that agent polling on a timer. Touching a sentinel file after
//! a write lets the agent react immediately (launchd `WatchPaths`,
//! inotify, `fswatch`, …) instead of waiting out its interval.
//!
//! The contract, deliberately narrow:
//!
//! - **One-way.** The server writes the sentinel and never reads it.
//!   Nothing about the server's behaviour depends on it, so an agent
//!   that is absent, stopped, or slow costs nothing but latency.
//! - **Only after a verified write** (GH #539). The sentinel means
//!   "something landed"; a failed or unverifiable write must leave it
//!   untouched, or the agent commits nothing and learns to distrust it.
//! - **Never fatal.** A sentinel that cannot be written is logged and
//!   ignored. Failing a write that already landed because a *hint* to
//!   an optional agent failed would be strictly worse than the polling
//!   it replaces.
//! - **Off by default**, enabled explicitly on `cdno-mcp-server`. The
//!   stdio binary never enables it: its client is a local session, and
//!   there is no agent on the other side of it.
//!
//! The default location is `<vault>/.git/cdno-sync.nudge`. Under `.git/`
//! on purpose — git will not track it and no sync tool that mirrors the
//! working tree will carry it, so the signal can never become content.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::server::CuadernoServer;

/// Filename of the sentinel inside the vault's `.git/`.
pub const SENTINEL_FILE_NAME: &str = "cdno-sync.nudge";

/// A configured sync-nudge sentinel. Cheap to clone-by-`Arc`; held by
/// the server and, in nudge-only checkpoint mode, by the sweep.
#[derive(Debug)]
pub struct SyncNudge {
    path: PathBuf,
    /// Whether a failure has already been reported at `warn`. A broken
    /// sentinel would otherwise log once per write; the first one is
    /// the operator's signal and the rest are noise.
    warned: AtomicBool,
}

impl SyncNudge {
    /// A sentinel at an explicit path.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            warned: AtomicBool::new(false),
        }
    }

    /// The default sentinel for a vault: `<root>/.git/cdno-sync.nudge`.
    pub fn default_path(vault_root: &Path) -> PathBuf {
        vault_root.join(".git").join(SENTINEL_FILE_NAME)
    }

    /// Where this sentinel lives — for the startup log, so an operator
    /// can point their agent at it without guessing.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Touch the sentinel: rewrite it so both its mtime and its content
    /// change, which is what the widest range of watchers actually
    /// notice (`launchd`'s `WatchPaths` keys off the former, some
    /// polling agents off the latter).
    ///
    /// The payload is a monotonically-increasing-ish unix timestamp in
    /// seconds — enough for an agent to log *when* it was woken, and
    /// deliberately not vault content: the sentinel names no note.
    ///
    /// Never fails. Parent directories are **not** created: the default
    /// path's parent is the repo's own `.git/`, and conjuring that
    /// directory into existence would be a worse outcome than the
    /// sentinel not working.
    pub fn touch(&self) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match std::fs::write(&self.path, format!("{stamp}\n")) {
            Ok(()) => tracing::debug!(sentinel = %self.path.display(), "sync nudge touched"),
            Err(e) => {
                // First failure at warn, the rest at debug: one write
                // per tool call would otherwise flood the log.
                if self.warned.swap(true, Ordering::Relaxed) {
                    tracing::debug!(error = %e, sentinel = %self.path.display(), "sync nudge failed again");
                } else {
                    tracing::warn!(
                        error = %e,
                        sentinel = %self.path.display(),
                        "could not touch the sync-nudge sentinel; writes still land, but the \
                         external sync agent will only notice them on its own timer"
                    );
                }
            }
        }
    }
}

impl CuadernoServer {
    /// Signal the external sync agent, if one is configured. Called
    /// from [`CuadernoServer::verified_write`] and nowhere else, so the
    /// "only after a verified write" rule holds by construction.
    pub(crate) fn nudge_sync_agent(&self) {
        if let Some(nudge) = self.sync_nudge() {
            nudge.touch();
        }
    }
}

/// Convenience alias for the shared handle the server and the
/// checkpoint sweep both hold.
pub type SharedNudge = Arc<SyncNudge>;
