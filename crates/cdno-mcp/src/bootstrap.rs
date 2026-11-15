//! Vault-opening for the two cdno-mcp binaries.
//!
//! The open logic itself now lives in `cdno_domain::bootstrap` — the
//! Tauri app became the fourth consumer, tripping the lift this
//! module's previous incarnation documented. What remains here is the
//! anyhow boundary the binaries want: library errors are typed
//! (`BootstrapError`), binaries speak `anyhow::Result`.

use std::ffi::OsStr;
use std::path::Path;

use anyhow::{Context, Result};

pub use cdno_domain::bootstrap::OpenedVault;

/// Open a vault at `root` — see [`cdno_domain::bootstrap::open_vault`].
pub fn open_vault(root: &Path) -> Result<OpenedVault> {
    cdno_domain::bootstrap::open_vault(root)
        .with_context(|| format!("opening vault at {}", root.display()))
}

/// Open a vault at `root` with the index override given explicitly — see
/// [`cdno_domain::bootstrap::open_vault_with_index`]. The HTTP server's
/// `--index-path` (which falls back to `CUADERNO_INDEX_PATH`) comes
/// through here; the stdio server reads the variable via [`open_vault`].
pub fn open_vault_with_index(root: &Path, index_override: Option<&OsStr>) -> Result<OpenedVault> {
    cdno_domain::bootstrap::open_vault_with_index(root, index_override)
        .with_context(|| format!("opening vault at {}", root.display()))
}
