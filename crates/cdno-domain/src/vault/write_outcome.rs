//! [`WriteOutcome`] — the richer return of write operations whose
//! callers must know *every* path a transaction touched, and whether it
//! touched anything at all.
//!
//! Most write methods return only their semantic "primary" path (the
//! project map rewritten, the note created). That is enough for the CLI
//! and MCP, which surface a single line of feedback. The desktop app is
//! different: its file-watcher echo journal must suppress the events for
//! *its own* writes, so it needs the exact path set the write produced —
//! not a client-side reconstruction that guesses at daily-note paths and
//! silently misses archival moves (#315). It also must distinguish a real
//! write from a silent no-op, or it plants a false echo-suppression entry
//! over paths that never changed.

use cdno_core::path::VaultPath;

/// The full effect of a write operation: its primary path plus every
/// path the committing transaction actually wrote.
///
/// `primary` is always present — even on a no-op — so CLI/MCP can keep
/// reporting a single path. `paths` is the journal-grade set: empty on a
/// no-op, otherwise a superset of `primary` (it also carries the
/// daily-log note and any archival move's source and destination).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutcome {
    /// The path the operation is semantically "about": the project map,
    /// the note created. What CLI/MCP put in their result message.
    pub primary: VaultPath,
    /// Every distinct path the committed transaction wrote, in commit
    /// order. Empty when the operation was a silent no-op — see
    /// [`touched`](Self::touched).
    pub paths: Vec<VaultPath>,
}

impl WriteOutcome {
    /// A real write: `primary` plus the transaction's full touched set.
    pub fn written(primary: VaultPath, paths: Vec<VaultPath>) -> Self {
        Self { primary, paths }
    }

    /// A silent no-op: the operation resolved a path but wrote nothing.
    /// `paths` is empty, so [`touched`](Self::touched) is `false` and the
    /// desktop layer knows to skip journalling and its self-change emit.
    pub fn noop(primary: VaultPath) -> Self {
        Self {
            primary,
            paths: Vec::new(),
        }
    }

    /// Whether the operation wrote anything. `false` marks a no-op (e.g.
    /// `update_project_state` with unchanged text): journalling it would
    /// suppress a genuine external edit to those paths for the echo
    /// window, so callers must not record or emit on `false`.
    pub fn touched(&self) -> bool {
        !self.paths.is_empty()
    }
}
