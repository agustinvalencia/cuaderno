//! Wire production dependencies (filesystem store, SQLite index,
//! TOML config) into a [`Vault`] and run startup reconciliation.
//!
//! Every subcommand that operates on an existing vault opens it
//! through here. `init` is the exception — it creates the vault
//! rather than opening one.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result, bail};
use cdno_core::config::VaultConfig;
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::paths;
use cdno_core::reconcile::ReconciliationReport;
use cdno_core::store::{FsVaultStore, VaultStore};
use cdno_domain::{
    MethodSkew, MethodStatus, Vault, describe_vault_method_version, newer_minor_message,
};

/// Walk upward from `start` looking for a `.cuaderno/` marker.
/// Returns the directory containing it, or `None` if no vault is found.
///
/// Mirrors the discovery semantics of `git`: a user can run `cdno`
/// from anywhere inside the vault and have it locate the root.
pub fn discover_vault_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(paths::CUADERNO_DIR).is_dir())
        .map(Path::to_path_buf)
}

/// Resolve which vault root to operate on, in precedence order:
///
/// 1. `vault_flag` — an explicit `--vault <path>`, wins unconditionally.
/// 2. A vault discovered by walking up from `cwd` — so running *inside*
///    a vault always targets that vault, even when `env_vault_path`
///    points elsewhere. Discovery must beat the env fallback or a stray
///    `CUADERNO_VAULT_PATH` would silently misroute writes in a
///    multi-vault setup.
/// 3. `env_vault_path` — the `CUADERNO_VAULT_PATH` value, the "I'm
///    outside any vault" fallback. Empty / whitespace-only values are
///    treated as unset so an accidentally-exported blank var doesn't
///    resolve to `""`.
///
/// Returns `None` when nothing resolves; the caller renders the error.
/// Pure over its inputs — `main` supplies the real CWD and environment
/// — so the precedence policy is testable without touching either.
pub fn resolve_vault_root(
    vault_flag: Option<&Path>,
    cwd: &Path,
    env_vault_path: Option<&str>,
) -> Option<PathBuf> {
    if let Some(path) = vault_flag {
        return Some(path.to_path_buf());
    }
    if let Some(root) = discover_vault_root(cwd) {
        return Some(root);
    }
    if let Some(value) = env_vault_path
        && !value.trim().is_empty()
    {
        return Some(PathBuf::from(value));
    }
    None
}

/// Open a Cuaderno vault rooted at `root`: load config, open the
/// SQLite index, run startup reconciliation.
///
/// Errors if `root/.cuaderno/` is missing — that signals the user
/// pointed at a non-vault directory and should run `cdno init`.
pub fn open_vault(root: &Path) -> Result<(Vault, ReconciliationReport)> {
    let cuaderno_dir = root.join(paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        bail!(
            "no Cuaderno vault at {}; run `cdno init` to create one.",
            root.display()
        );
    }

    let config = VaultConfig::load(root)
        .with_context(|| format!("loading {}", root.join(paths::CONFIG_FILE).display()))?;

    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> = Arc::new(
        SqliteIndex::open(root.join(paths::INDEX_DB))
            .with_context(|| format!("opening {}", root.join(paths::INDEX_DB).display()))?,
    );

    let (vault, report) =
        Vault::new(store, index, config).context("constructing vault and reconciling index")?;

    // Surface per-file reconciliation errors (#207). A note that fails
    // to parse is dropped from the index and otherwise invisible -- every
    // command opens the vault through here, so warning once at the
    // boundary catches a corrupt note on the next invocation. Clean
    // vaults stay silent.
    if !report.errors.is_empty() {
        eprintln!(
            "warning: {} note(s) could not be indexed during reconciliation:",
            report.errors.len()
        );
        for issue in &report.errors {
            eprintln!("  {}: {}", issue.path, issue.reason);
        }
    }

    // Record the vault's method version for the skew notice, never print
    // it: shell completion opens the vault through here on every Tab, so a
    // notice printed here would smear across the prompt. `main` prints it
    // after the command, from the dispatch (RFC 0006 plan T9).
    if let Ok(status) = vault.method_status() {
        record_method_status(status);
    }

    Ok((vault, report))
}

/// The method status of the first vault this process opened, held for the
/// skew notice `main` prints after a command has run. Process-global because
/// every command opens its own vault inside its handler; the first open
/// wins, so a command that opens the vault twice is told once.
static METHOD_STATUS: Mutex<Option<MethodStatus>> = Mutex::new(None);

fn record_method_status(status: MethodStatus) {
    let mut slot = METHOD_STATUS.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(status);
    }
}

/// Take the method status recorded by [`open_vault`], if a vault was opened
/// since the last take. `None` when no vault was opened, or its version
/// could not be read (the open itself reports a malformed version).
pub fn take_method_status() -> Option<MethodStatus> {
    METHOD_STATUS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

/// The one-line skew notice for `status`, or `None` when there is nothing to
/// say. Two cases, neither with a count of anything (RFC 0006 §8.1: counting
/// on every invocation is the wrong cost):
///
/// - the vault's effective version (unstamped reads as 1.0) is below this
///   binary's: it names both versions and points at `cdno migrate --dry-run`;
/// - the vault is on a newer minor: worded as `cdno lint`'s warning, since
///   writes proceed (the gate allows a newer minor).
///
/// A newer major prints nothing here: every write is refused with the gate's
/// own message, which already names the remedy.
pub fn method_notice(status: &MethodStatus) -> Option<String> {
    let (vault, binary) = (status.vault, status.binary);
    match status.skew {
        MethodSkew::OlderMinor | MethodSkew::OlderMajor => Some(format!(
            "notice: this vault {}; this cdno implements RLM {binary} \u{2014} \
             `cdno migrate --dry-run` shows what changes",
            describe_vault_method_version(vault)
        )),
        MethodSkew::NewerMinor => Some(format!("notice: {}", newer_minor_message(vault, binary))),
        MethodSkew::Equal | MethodSkew::NewerMajor => None,
    }
}
