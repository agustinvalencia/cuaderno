//! Re-run reconciliation on an open vault.
//!
//! [`Vault::new`] reconciles at construction — a documented invariant, so
//! every domain method may assume the index matches the filesystem on
//! entry. That is enough for a process that opens the vault, does one
//! thing and exits, which is every CLI verb but one.
//!
//! `cdno watch` (#600) is the exception: it stays alive while the
//! filesystem changes underneath it, so it needs to reconcile again
//! without tearing down and rebuilding the vault. Reopening would mean a
//! second `SqliteIndex` handle on the same file and a fresh config load
//! per batch; this is the same pass `Vault::new` runs, on the store and
//! index already held.
//!
//! The ignore set is recompiled from the config the vault was opened
//! with, NOT re-read from disk. A config edited while `cdno watch` runs
//! therefore does not take effect until it is restarted — deliberate, and
//! the honest boundary for a reconcile-only loop: honouring a new
//! `ignore` glob means rebuilding the vault, which is the desktop's
//! `reload_vault_config` path and the one #459 describes racing. Saying
//! "restart to pick up a config change" is a limitation; silently
//! reconciling against stale globs would be a bug.

use cdno_core::reconcile::{ReconciliationReport, reconcile};

use crate::error::DomainError;
use crate::vault::Vault;

impl Vault {
    /// Reconcile the index against the filesystem again, returning the
    /// pass's report.
    ///
    /// Idempotent and safe to call repeatedly: the index is a cache
    /// derived entirely from the notes on disk, so a redundant pass costs
    /// time and nothing else. Per-file failures accumulate into the
    /// report rather than aborting, so one unparseable note does not stop
    /// the rest of the vault being indexed.
    pub fn reconcile(&self) -> Result<ReconciliationReport, DomainError> {
        let ignore = self.config.ignore_set()?;
        Ok(reconcile(&self.store, &self.index, &ignore)?)
    }
}
