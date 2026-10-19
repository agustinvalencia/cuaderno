//! The vault's method version, read from disk (RFC 0006 §8.4, plan T7a).
//!
//! The version is read from `.cuaderno/config.toml` through the store every
//! time, never from the [`VaultConfig`](cdno_core::config::VaultConfig)
//! loaded at open: the remote server reads its config once at start, and
//! still has to see a vault that `cdno migrate` has moved on since.
//! `Vault::transaction()` applies the gate to this read, taken under the
//! write lock; [`Vault::method_status`] reports it.
//!
//! A vault on a newer minor version is allowed, and made visible once per
//! `Vault` through [`Vault::take_method_notice`] (plan T7b); a binary opens
//! one `Vault` per start, so that is once per start.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

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

/// The newer-minor skew notice: recorded by `Vault::new` from the config
/// loaded at open and again by the write gate whenever its re-read finds a
/// newer minor, and yielded once per `Vault` by
/// [`Vault::take_method_notice`].
///
/// Both halves are `Sync` (a `Mutex` and an `AtomicBool`, never a `Cell`):
/// `cdno-mcp` shares the `Vault` as `Arc<Vault>` across `spawn_blocking`.
#[derive(Debug, Default)]
pub(in crate::vault) struct MethodNotice {
    /// The latest newer-minor status recorded, if any.
    pending: Mutex<Option<MethodStatus>>,
    /// Set once the notice has been yielded; never reset.
    taken: AtomicBool,
}

impl MethodNotice {
    /// Record `vault` against this binary's version when it is a newer
    /// minor; any other skew records nothing.
    pub(in crate::vault) fn record(&self, vault: Option<MethodVersion>) {
        let skew = MethodSkew::between(METHOD_VERSION, vault);
        if skew == MethodSkew::NewerMinor {
            *self.pending.lock().unwrap_or_else(PoisonError::into_inner) = Some(MethodStatus {
                vault,
                binary: METHOD_VERSION,
                skew,
            });
        }
    }
}

impl Vault {
    /// The newer-minor skew notice, once per `Vault`: `Some` the first
    /// time it is asked for after a newer minor has been recorded (at open,
    /// or by a write's re-read), `None` before that and ever after. The
    /// MCP binaries log it at start.
    pub fn take_method_notice(&self) -> Option<MethodStatus> {
        let pending = (*self
            .method_notice
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner))?;
        (!self.method_notice.taken.swap(true, Ordering::AcqRel)).then_some(pending)
    }

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
