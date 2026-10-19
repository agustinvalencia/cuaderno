//! The vault's method version, read from disk (RFC 0006 §8.4, plan T7a).
//!
//! The version is read from `.cuaderno/config.toml` through the store every
//! time, never from the [`VaultConfig`](cdno_core::config::VaultConfig)
//! loaded at open: the remote server reads its config once at start, and
//! still has to see a vault that `cdno migrate` has moved on since.
//! `Vault::transaction()` applies the gate to this read, taken under the
//! write lock; [`Vault::method_status`] reports it.

use cdno_core::config::{MethodVersion, method_version_from_str};

use super::Vault;
use super::config::read_config_from;
use crate::error::DomainError;
use crate::method_version::{METHOD_VERSION, MethodSkew};

/// How the vault's recorded method version stands against this binary's,
/// as read from disk now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodStatus {
    /// The vault's `[vault] method_version`; `None` when unstamped (no
    /// config file, or no key).
    pub vault: Option<MethodVersion>,
    /// The version this binary implements, [`METHOD_VERSION`].
    pub binary: MethodVersion,
    /// The comparison, with an unstamped vault read as 1.0.
    pub skew: MethodSkew,
}

impl Vault {
    /// The vault's method version against this binary's, read from disk
    /// the same way the write gate reads it. What the skew notice, lint and
    /// `cdno migrate` consult, so no interface crate compares versions
    /// itself.
    pub fn method_status(&self) -> Result<MethodStatus, DomainError> {
        let vault = self.recorded_method_version()?;
        Ok(MethodStatus {
            vault,
            binary: METHOD_VERSION,
            skew: MethodSkew::between(METHOD_VERSION, vault),
        })
    }

    /// Read `[vault] method_version` alone from `.cuaderno/config.toml`
    /// through the store.
    ///
    /// No config file, or no key, is `Ok(None)` (unstamped), as
    /// [`read_config_from`] already reads absence as empty. Every other key
    /// is ignored, so an unrelated config problem does not block a write. A
    /// file that is no longer TOML, or a malformed version, is
    /// [`ConfigError::MethodVersion`](cdno_core::error::ConfigError::MethodVersion),
    /// whose message names the file, the key and the reason: never a
    /// fallback to the version loaded at open.
    pub(in crate::vault) fn recorded_method_version(
        &self,
    ) -> Result<Option<MethodVersion>, DomainError> {
        let document = read_config_from(&*self.store)?;
        Ok(method_version_from_str(&document.content)?)
    }
}
