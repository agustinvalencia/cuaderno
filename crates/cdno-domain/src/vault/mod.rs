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
mod config;
mod context;
mod custom_notes;
mod daily;
mod index_entry;
mod links;
mod lint;
mod log;
mod monthly;
mod normalise;
mod notes;
mod orient;
mod portfolios;
mod projects;
mod questions;
mod search;
mod set_frontmatter;
pub(crate) mod slug;
mod slug_hint;
mod stewardships;
mod templating;
mod tracking;
mod weekly;
mod write_outcome;

/// The level-2 heading of a daily note's running history. Shared across
/// the modules that read it (`context`), write it (`log`), and keep it
/// pinned to the bottom (`daily`) so the heading text lives in one place.
pub(in crate::vault) const DAILY_LOGS_SECTION: &str = "Logs";

/// The ISO-week label `YYYY-Www` for the week containing `date` — e.g.
/// `2026-W01`. The year is the ISO week-numbering year (which can differ
/// from the calendar year around a year boundary, so 2025-12-29 is
/// `2026-W01`), and the week is zero-padded to two digits.
///
/// Shared so the daily scaffold (`log`) and the weekly scaffold
/// (`weekly`) render the `week` placeholder identically — the daily
/// note's `week:` frontmatter must match the weekly note it points at.
pub(in crate::vault) fn iso_week_label(date: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let iso = date.iso_week();
    format!("{}-W{:02}", iso.year(), iso.week())
}

pub use capture::InboxItem;
pub use commitments::{CommitmentEntry, CommitmentSource};
pub use config::{ConfigDocument, ConfigSaveError, ConfigValidationError, validate_config_str};
pub use context::{
    CompletedActionEntry, DailyLogLine, ProjectBacklinks, ProjectStateChange, TrackingEntry,
    TrackingPoint, TrackingSeries,
};
// Timezone-injected staleness helpers, hidden from the public API but
// reachable by the deterministic staleness test (#380).
#[doc(hidden)]
pub use context::{days_since_mtime_in, mtime_threshold_ns_in};
pub use daily::{DailyNoteView, DailySection};
pub use links::ResolvedLink;
pub use monthly::{MonthlyNoteView, MonthlySection};
pub use normalise::NormaliseReport;
pub use notes::NoteView;
pub use orient::{LapsedHabit, OrientationContext};
pub use portfolios::PortfolioSummary;
pub use projects::{ActionListEntry, AttachedAction, ProjectSummary, TopAction};
pub use questions::QuestionSummary;
pub use search::{SearchFilters, SearchResultEntry};
pub use stewardships::{StewardshipSummary, StewardshipVariant};
pub use templating::{
    PlaceholderSource, TemplateContent, TemplatePlaceholder, TemplateSourceKind, TemplateSummary,
};
pub use weekly::{WeeklyNoteView, WeeklySection};
pub use write_outcome::WriteOutcome;

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
        // Compile the config `ignore` globs once and hand the matcher to
        // reconciliation. A malformed pattern surfaces here, at vault
        // open, rather than silently skipping the rule.
        let ignore = config.ignore_set()?;
        // Validate `[note_types.*]` at vault-open so a malformed or built-in-
        // shadowing custom type fails fast, not mid-operation.
        crate::type_registry::TypeRegistry::validate(&config)?;
        let report = reconcile(&store, &index, &ignore)?;
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

    /// A registry view over this vault's built-in and config-defined note
    /// types. Cheap to build (borrows the config); the config was validated at
    /// [`Vault::new`], so callers can treat every resolved descriptor as sound.
    pub fn type_registry(&self) -> crate::type_registry::TypeRegistry<'_> {
        crate::type_registry::TypeRegistry::new(&self.config)
    }

    /// Start an uncommitted transaction bound to this vault's store
    /// and index. Operation submodules use this to enqueue a batch
    /// of writes and call `commit()` when ready.
    /// Acquires the vault write lock up front (#196), so the returned
    /// transaction must be created *before* a write op reads the file it
    /// is about to rewrite — that is what serialises the read-modify-write
    /// against other processes. Hence `Result`.
    pub(in crate::vault) fn transaction(&self) -> Result<VaultTransaction, DomainError> {
        Ok(VaultTransaction::new(
            Arc::clone(&self.store),
            Arc::clone(&self.index),
        )?)
    }
}
