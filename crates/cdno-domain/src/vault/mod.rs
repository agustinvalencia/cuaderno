//! [`Vault`] — the domain-layer entry point that stitches a
//! [`VaultStore`], a [`VaultIndex`], and a [`VaultConfig`] into a
//! single object downstream crates depend on.
//!
//! The store and index are held as `Arc<dyn _>` trait objects. One
//! reason over monomorphisation: uniformity with `VaultTransaction`,
//! which already uses `Arc<dyn>`. See
//! `Projects/cuaderno/resources/decision-vault-generics-vs-dyn.md`
//! for the full rationale.
//!
//! Startup reconciliation runs inside [`Vault::new`] so any domain
//! method can assume the index reflects the filesystem on entry.
//!
//! # Where the operations live
//!
//! High-level operations are split across feature submodules so this
//! file stays small. Each submodule attaches an `impl Vault { ... }`
//! block. Add a new operation by creating a new file rather than
//! growing this one — the existing `log` and `lint` submodules show
//! the pattern.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::VaultIndex;
use cdno_core::reconcile::{ReconciliationReport, reconcile};
use cdno_core::store::VaultStore;
use cdno_core::transaction::VaultTransaction;

use crate::error::DomainError;

mod actions;
mod capture;
mod commitments;
mod index_entry;
mod lint;
mod log;
mod orient;
mod projects;
mod slug;

pub use commitments::{CommitmentEntry, CommitmentSource};
pub use orient::{LapsedHabit, OrientationContext};
pub use projects::{ActionListEntry, AttachedAction, ProjectSummary, TopAction};

// Re-exported for the targeted test in `tests/unit/projects_tests.rs`
// to reach the helper's defensive error branches without any
// `#[cfg(test)] mod tests` block in the source. External callers
// other than tests should not depend on this — it's a domain-internal
// frontmatter mutator.
pub use projects::rewrite_field_in_frontmatter;

/// Domain entry point. Owns the store, index, and config; hands out
/// transactions; exposes high-level operations defined in feature
/// submodules.
pub struct Vault {
    pub(in crate::vault) store: Arc<dyn VaultStore>,
    pub(in crate::vault) index: Arc<dyn VaultIndex>,
    pub(in crate::vault) config: VaultConfig,
}

impl Vault {
    /// Construct a vault and run startup reconciliation. The returned
    /// [`ReconciliationReport`] lets callers surface scan counts or
    /// per-file issues without re-running the pass.
    pub fn new(
        store: Arc<dyn VaultStore>,
        index: Arc<dyn VaultIndex>,
        config: VaultConfig,
    ) -> Result<(Self, ReconciliationReport), DomainError> {
        let report = reconcile(&store, &index)?;
        Ok((
            Self {
                store,
                index,
                config,
            },
            report,
        ))
    }

    pub fn config(&self) -> &VaultConfig {
        &self.config
    }

    /// Start an uncommitted transaction bound to this vault's store
    /// and index. Operation submodules use this to enqueue a batch
    /// of writes and call `commit()` when ready.
    pub(in crate::vault) fn transaction(&self) -> VaultTransaction {
        VaultTransaction::new(Arc::clone(&self.store), Arc::clone(&self.index))
    }
}
