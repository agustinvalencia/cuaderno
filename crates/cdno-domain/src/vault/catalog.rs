//! [`Catalog`] — the vault's note listings read from its files, with no index.
//!
//! Shell completion re-invokes the binary on every Tab, and opening the index
//! is a write: it migrates and reconciles, and on exit SQLite checkpoints and
//! removes the `-wal` and `-shm` files. Beside a long-lived server whose locks
//! this process cannot see, that write damages the server's index (#1007), as
//! a prompt hook running `cdno now` did (#1003, answered by [`super::Journal`]).
//!
//! The completers asked a freshly reconciled index, so their answers were
//! already a function of the files. A catalog computes the same function
//! without the cache: the classifier says which files are notes
//! ([`cdno_core::classify::classify`]), reconciliation's own admission check
//! says which of them an index holds and with which type
//! ([`cdno_core::reconcile::admit`]), and the listing folds are the ones
//! [`Vault`] runs over its index rows. So for an up-to-date vault a catalog
//! and a vault list the same notes, the same way. Nothing is written: the
//! store is only read, and no index or lock file is opened.

use std::sync::{Arc, OnceLock};

use chrono::NaiveDate;

use cdno_core::classify::classify;
use cdno_core::config::VaultConfig;
use cdno_core::path::VaultPath;
use cdno_core::reconcile::admit;
use cdno_core::store::VaultStore;

use crate::error::DomainError;
use crate::frontmatter::ProjectFrontmatter;
use crate::frontmatter::ProjectStatus;
use crate::note_type::NoteType;
use crate::type_registry::TypeRegistry;

#[cfg(doc)]
use super::Vault;
use super::portfolios::{PortfolioSummary, portfolio_summaries};
use super::projects::{closed_maps_in, projects_with_status};
use super::questions::{QuestionSummary, question_summaries};
use super::stewardships::{StewardshipSummary, stewardship_summaries};
use super::{note_scope_for, validate_config_at_open};

/// The listings shell completion offers, answered from the files alone, each
/// exactly as [`Vault`] answers it from an up-to-date index.
pub struct Catalog {
    store: Arc<dyn VaultStore>,
    config: VaultConfig,
    /// Every note an up-to-date index would hold, with its type, in the
    /// index's path order. Read once, on first use.
    notes: OnceLock<Vec<(VaultPath, String)>>,
}

impl Catalog {
    /// A catalog over `store`, with the vault's `config`.
    ///
    /// `config` goes through the same checks as [`Vault::new`], so a vault
    /// whose config refuses to open refuses here too.
    pub fn new(store: Arc<dyn VaultStore>, mut config: VaultConfig) -> Result<Self, DomainError> {
        validate_config_at_open(&mut config)?;
        Ok(Self {
            store,
            config,
            notes: OnceLock::new(),
        })
    }

    /// The built-in and config-defined note types: [`Vault::type_registry`].
    pub fn type_registry(&self) -> TypeRegistry<'_> {
        TypeRegistry::new(&self.config)
    }

    /// Every note an up-to-date index holds, in path order: the paths of
    /// [`Vault::list_note_candidates`].
    pub fn note_paths(&self) -> Result<Vec<VaultPath>, DomainError> {
        Ok(self.notes()?.iter().map(|(path, _)| path.clone()).collect())
    }

    /// [`Vault::active_projects`], from the files.
    pub fn active_projects(&self) -> Result<Vec<(VaultPath, ProjectFrontmatter)>, DomainError> {
        projects_with_status(
            &*self.store,
            self.paths_of(NoteType::Project)?,
            ProjectStatus::Active,
        )
    }

    /// [`Vault::parked_projects`], from the files.
    pub fn parked_projects(&self) -> Result<Vec<(VaultPath, ProjectFrontmatter)>, DomainError> {
        projects_with_status(
            &*self.store,
            self.paths_of(NoteType::Project)?,
            ProjectStatus::Parked,
        )
    }

    /// [`Vault::closed_projects`], from the files.
    pub fn closed_projects(&self) -> Result<Vec<(VaultPath, ProjectFrontmatter)>, DomainError> {
        Ok(
            closed_maps_in(&*self.store, self.paths_of(NoteType::Project)?)?
                .into_iter()
                .map(|(path, frontmatter, _title)| (path, frontmatter))
                .collect(),
        )
    }

    /// [`Vault::list_portfolios`], from the files.
    pub fn list_portfolios(&self, today: NaiveDate) -> Result<Vec<PortfolioSummary>, DomainError> {
        portfolio_summaries(
            &*self.store,
            self.paths_of(NoteType::Portfolio)?,
            &self.paths_of(NoteType::Evidence)?,
            today,
        )
    }

    /// [`Vault::list_stewardships`], from the files.
    pub fn list_stewardships(
        &self,
        today: NaiveDate,
    ) -> Result<Vec<StewardshipSummary>, DomainError> {
        stewardship_summaries(
            &*self.store,
            self.paths_of(NoteType::Stewardship)?,
            &self.paths_of(NoteType::Tracking)?,
            today,
        )
    }

    /// [`Vault::list_questions`], from the files.
    pub fn list_questions(&self) -> Result<Vec<QuestionSummary>, DomainError> {
        question_summaries(&*self.store, self.paths_of(NoteType::Question)?)
    }

    /// The notes of `note_type`, in path order: what `list_by_type` returns
    /// from an up-to-date index.
    fn paths_of(&self, note_type: NoteType) -> Result<Vec<VaultPath>, DomainError> {
        Ok(self
            .notes()?
            .iter()
            .filter(|(_, ty)| ty == note_type.as_str())
            .map(|(path, _)| path.clone())
            .collect())
    }

    /// The notes reconciliation would index, with their types: the
    /// classifier's notes whose bytes [`admit`] accepts. A note that cannot be
    /// read, or that `admit` refuses, is left out, as reconciliation leaves it
    /// out of the index. Sorted by the path's text, as the index orders `path`.
    fn notes(&self) -> Result<&[(VaultPath, String)], DomainError> {
        if let Some(notes) = self.notes.get() {
            return Ok(notes);
        }
        let scope = note_scope_for(&self.config)?;
        let classified = classify(&self.store, &scope)?;
        let mut notes = Vec::with_capacity(classified.notes.len());
        for path in classified.notes {
            let Ok(content) = self.store.read_file(&path) else {
                continue;
            };
            if let Ok(admitted) = admit(&path, &content, &scope.typst_types) {
                notes.push((path, admitted.note_type));
            }
        }
        notes.sort_by_cached_key(|(path, _)| path.to_string());
        Ok(self.notes.get_or_init(|| notes))
    }
}
