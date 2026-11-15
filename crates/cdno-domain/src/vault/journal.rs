//! [`Journal`] — the focus readers over a store alone, with no index.
//!
//! What you are on, and what you paused, is read back from the `## Logs`
//! of daily notes whose paths follow from their dates, under the `[focus]`
//! windows. Nothing on that path consults the index, so a caller that
//! needs only the focus (`cdno now`, which a prompt hook runs on every
//! turn) need not open, migrate or reconcile it: opening the index is a
//! write, and a hook beside a long-lived server must not write it.
//!
//! [`Vault`] answers the same questions by delegating here (through its
//! crate-private `journal()`), so the fold lives once, in `context.rs`, and the
//! two cannot disagree.

use std::sync::Arc;

use cdno_core::config::{FocusConfig, VaultConfig};
use cdno_core::store::VaultStore;

use crate::error::DomainError;

use super::method_status::{MethodStatus, method_status_in};
use super::{Vault, validate_config_at_open};

/// The daily log read without an index: the focus and the open pauses
/// (`current_focus`, `open_pauses`, `last_paused`), and the vault's method
/// status, each answered exactly as [`Vault`] answers it.
pub struct Journal {
    pub(in crate::vault) store: Arc<dyn VaultStore>,
    pub(in crate::vault) focus: FocusConfig,
}

impl Journal {
    /// A journal over `store`, with the windows from `config`.
    ///
    /// `config` goes through the same checks as [`Vault::new`]
    /// (`validate_config_at_open`, which both call), so a vault whose config
    /// refuses to open as a [`Vault`] refuses here too: skipping the index
    /// must not make a broken config readable.
    pub fn new(store: Arc<dyn VaultStore>, mut config: VaultConfig) -> Result<Self, DomainError> {
        validate_config_at_open(&mut config)?;
        Ok(Self {
            store,
            focus: config.focus,
        })
    }

    /// The vault's method version against this binary's, read from disk:
    /// [`Vault::method_status`] without the index.
    pub fn method_status(&self) -> Result<MethodStatus, DomainError> {
        method_status_in(&*self.store)
    }
}

impl Vault {
    /// This vault's [`Journal`]: its store and `[focus]` windows, which the
    /// focus readers on `Vault` delegate to.
    pub(crate) fn journal(&self) -> Journal {
        Journal {
            store: Arc::clone(&self.store),
            focus: self.config.focus.clone(),
        }
    }
}
