//! Vault-opening for the two cdno-mcp binaries.
//!
//! The open logic itself now lives in `cdno_domain::bootstrap` — the
//! Tauri app became the fourth consumer, tripping the lift this
//! module's previous incarnation documented. What remains here is the
//! anyhow boundary the binaries want: library errors are typed
//! (`BootstrapError`), binaries speak `anyhow::Result`.

use std::path::Path;

use anyhow::{Context, Result};

pub use cdno_domain::bootstrap::OpenedVault;

/// Open a vault at `root` — see [`cdno_domain::bootstrap::open_vault`].
pub fn open_vault(root: &Path) -> Result<OpenedVault> {
    cdno_domain::bootstrap::open_vault(root)
        .with_context(|| format!("opening vault at {}", root.display()))
}
