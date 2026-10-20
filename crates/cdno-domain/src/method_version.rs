//! The version of the Research Logbook Method this binary implements
//! (RFC 0006 §7.4, §8.4).
//!
//! The method version tracks the method's text, not cdno's crate version.
//! `cdno init` stamps a new vault with it, and it is what a vault's own
//! `[vault] method_version` is compared against. A binary never stamps a
//! version whose practices it lacks, so this stayed at 1.0 until parked
//! maps accepted writes and handing off was in place, and is 1.1 since
//! (RFC 0006 plan, T23; decision D4): parking clarified, practice 8 handing
//! off, and the monthly loop.

use cdno_core::config::MethodVersion;

/// The Research Logbook Method version this binary implements.
pub const METHOD_VERSION: MethodVersion = MethodVersion { major: 1, minor: 1 };

/// The version an unstamped vault (no config file, or no
/// `[vault] method_version` key) is read as when it is compared with a
/// binary's. Unstamped stays distinct from a vault stamped 1.0 wherever the
/// difference is recorded; only the comparison reads the two alike.
pub const UNSTAMPED_READS_AS: MethodVersion = MethodVersion { major: 1, minor: 0 };

/// A vault's recorded method version in words: `RLM 1.0`, or
/// `unstamped (read as 1.0)` for a vault that records none, so an unstamped
/// vault is never said to be at 1.0. The one owner of that wording; the
/// sentence forms build on it.
pub fn describe_method_version(vault: Option<MethodVersion>) -> String {
    match vault {
        Some(version) => format!("RLM {version}"),
        None => format!("unstamped (read as {UNSTAMPED_READS_AS})"),
    }
}

/// The sentence for a vault on a newer minor version than `binary`: writes
/// proceed, and upgrading cdno brings the practices it adds. `cdno lint`'s
/// warning and the CLI's skew notice both say it, so it has one owner.
pub fn newer_minor_message(vault: Option<MethodVersion>, binary: MethodVersion) -> String {
    format!(
        "this vault {}, a newer minor version than the RLM {binary} this cdno implements \
         \u{2014} writes proceed; upgrade cdno for the practices it adds",
        crate::error::describe_vault_method_version(vault)
    )
}

/// How a vault's method version stands against a binary's: the one
/// comparison, so no interface crate compares versions itself.
///
/// An unstamped vault is compared as [`UNSTAMPED_READS_AS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodSkew {
    /// The vault is on the binary's version.
    Equal,
    /// The vault is on an older minor version of the binary's major.
    OlderMinor,
    /// The vault is on a newer minor version of the binary's major.
    NewerMinor,
    /// The vault is on an older major version.
    OlderMajor,
    /// The vault is on a newer major version.
    NewerMajor,
}

impl MethodSkew {
    /// Compare a vault's recorded version (`None` when unstamped) with a
    /// binary's.
    pub fn between(binary: MethodVersion, vault: Option<MethodVersion>) -> Self {
        use std::cmp::Ordering;
        let vault = vault.unwrap_or(UNSTAMPED_READS_AS);
        match (
            vault.major.cmp(&binary.major),
            vault.minor.cmp(&binary.minor),
        ) {
            (Ordering::Greater, _) => MethodSkew::NewerMajor,
            (Ordering::Less, _) => MethodSkew::OlderMajor,
            (Ordering::Equal, Ordering::Greater) => MethodSkew::NewerMinor,
            (Ordering::Equal, Ordering::Less) => MethodSkew::OlderMinor,
            (Ordering::Equal, Ordering::Equal) => MethodSkew::Equal,
        }
    }
}

/// Whether a write may proceed in a vault, as [`method_gate`] decides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Writes proceed: the vault is on the binary's major version (a newer
    /// or an older minor, unstamped, or equal).
    Allow,
    /// The vault is on a newer major version than the binary: writes are
    /// refused and reads keep working. The remedy is to upgrade cdno.
    MethodVersionAhead,
    /// The vault is on an older major version than the binary: writes are
    /// refused until the vault is migrated (`cdno migrate --dry-run`).
    VaultNeedsMigration,
}

/// Decide whether a binary implementing `binary` may write to a vault that
/// records `vault` (`None` when unstamped, compared as
/// [`UNSTAMPED_READS_AS`]). Pure: `Vault::transaction()` applies it to the
/// version it re-reads from disk under the write lock (RFC 0006 §8.4).
pub fn method_gate(binary: MethodVersion, vault: Option<MethodVersion>) -> Gate {
    match MethodSkew::between(binary, vault) {
        MethodSkew::NewerMajor => Gate::MethodVersionAhead,
        MethodSkew::OlderMajor => Gate::VaultNeedsMigration,
        MethodSkew::Equal | MethodSkew::OlderMinor | MethodSkew::NewerMinor => Gate::Allow,
    }
}
