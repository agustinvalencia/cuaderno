//! The tool calls a server has running, so a stdio server can wait for
//! them before it exits (#1002).
//!
//! Every tool's blocking work passes through here: the domain calls by way
//! of `CuadernoServer::with_vault` ([`ToolCalls::run`], counted), and the
//! two index-health tools, which touch only the index
//! ([`ToolCalls::run_index_only`], within the limit, uncounted). A call is counted from the moment it asks to run until
//! its blocking closure returns, so a call whose async half was dropped
//! (rmcp gave up on it after the client disconnected) is still counted
//! while its closure runs: the closure is what writes, and a write cut
//! inside `VaultTransaction::commit`, between two file writes, leaves the
//! notes half-changed with nothing to roll it back. [`ToolCalls::idle`]
//! waits for the count to reach zero.
//!
//! An optional limit bounds how many closures run at once. The stdio
//! server sets one below its blocking-pool size, so the stdin reader,
//! stdout writes and a sweep always find a thread; the HTTP server bounds
//! requests at the transport instead and sets none.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::{Notify, Semaphore};

/// The tool calls one server has running.
#[derive(Debug, Default)]
pub struct ToolCalls {
    running: AtomicUsize,
    idle: Notify,
    limit: Option<Arc<Semaphore>>,
}

impl ToolCalls {
    /// No limit on how many run at once.
    pub fn new() -> Self {
        Self::default()
    }

    /// At most `limit` closures run at once; the others wait their turn,
    /// counted.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit: Some(Arc::new(Semaphore::new(limit))),
            ..Self::default()
        }
    }

    /// How many calls are counted now: running, or waiting for a turn.
    pub fn in_flight(&self) -> usize {
        self.running.load(Ordering::Acquire)
    }

    /// Wait until no call is counted.
    pub async fn idle(&self) {
        loop {
            let notified = self.idle.notified();
            tokio::pin!(notified);
            // Registered before the count is read, so a call finishing in
            // between still wakes this waiter.
            notified.as_mut().enable();
            if self.in_flight() == 0 {
                return;
            }
            notified.await;
        }
    }

    /// Run `f` on the blocking pool, counted until it returns.
    pub async fn run<R>(
        self: &Arc<Self>,
        f: impl FnOnce() -> R + Send + 'static,
    ) -> Result<R, tokio::task::JoinError>
    where
        R: Send + 'static,
    {
        self.run_inner(f, true).await
    }

    /// Run `f` on the blocking pool within the limit but uncounted: for
    /// work that touches only the index (`index_status`, `rebuild_index`),
    /// which a shutdown may cut like a sweep, since SQLite rolls it back
    /// and the next open reconciles.
    pub async fn run_index_only<R>(
        self: &Arc<Self>,
        f: impl FnOnce() -> R + Send + 'static,
    ) -> Result<R, tokio::task::JoinError>
    where
        R: Send + 'static,
    {
        self.run_inner(f, false).await
    }

    async fn run_inner<R>(
        self: &Arc<Self>,
        f: impl FnOnce() -> R + Send + 'static,
        count: bool,
    ) -> Result<R, tokio::task::JoinError>
    where
        R: Send + 'static,
    {
        let counted = count.then(|| {
            self.running.fetch_add(1, Ordering::AcqRel);
            Counted(Arc::clone(self))
        });
        let permit = match &self.limit {
            // A closed semaphore cannot happen (it is never closed); run
            // without a permit rather than fail the call if it ever did.
            Some(limit) => Arc::clone(limit).acquire_owned().await.ok(),
            None => None,
        };
        tokio::task::spawn_blocking(move || {
            // Both move into the closure, so the count and the turn are
            // held for as long as the work runs, not as long as somebody
            // awaits it.
            let _counted = counted;
            let _permit = permit;
            f()
        })
        .await
    }
}

/// Uncounts its call when dropped: when the closure returns, or, for a
/// call dropped while still waiting for a turn, then.
struct Counted(Arc<ToolCalls>);

impl Drop for Counted {
    fn drop(&mut self) {
        if self.0.running.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}
