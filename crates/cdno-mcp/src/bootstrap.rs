//! Shared vault-opening for the two cdno-mcp binaries.
//!
//! Both `bin/stdio.rs` and `bin/server.rs` previously carried their
//! own near-copies of `cdno_cli::bootstrap::open_vault` (duplicated so
//! cdno-mcp never depends on the CLI crate); the HTTP binary's copy
//! then diverged — it also needs the store/index/ignore handles for
//! its reconciliation loop. That divergence tripped the "if this
//! drift becomes painful, lift it" wire, so the open logic now lives
//! here once and each binary takes what it needs.
//!
//! The CLI's copy intentionally stays where it is: its surrounding
//! *resolution* semantics (upward vault discovery, `--vault`/env
//! precedence) are CLI-specific UX. If a fourth copy ever threatens,
//! the next lift is into `cdno-domain` (NOT `cdno-core` — this
//! function constructs a [`Vault`], and the domain crate sits above
//! core in the dependency order).

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use cdno_core::config::{IgnoreSet, VaultConfig};
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::paths;
use cdno_core::reconcile::ReconciliationReport;
use cdno_core::store::{FsVaultStore, VaultStore};
use cdno_domain::Vault;

/// Everything [`open_vault`] produced. The store/index/ignore handles
/// exist so a long-running caller (the HTTP server's reconciliation
/// loop) can re-run the pass that [`Vault::new`] performs once at
/// open — `Vault` deliberately does not re-expose them. Short-lived
/// callers (the stdio binary) just take `vault` and drop the rest.
pub struct OpenedVault {
    pub vault: Vault,
    pub store: Arc<dyn VaultStore>,
    pub index: Arc<dyn VaultIndex>,
    pub ignore: Arc<IgnoreSet>,
    pub report: ReconciliationReport,
}

/// Open a vault at `root`: check the `.cuaderno/` marker, load config,
/// wire the filesystem store + SQLite index, and run the startup
/// reconciliation via [`Vault::new`].
pub fn open_vault(root: &Path) -> Result<OpenedVault> {
    let cuaderno_dir = root.join(paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        bail!(
            "no Cuaderno vault at {} (looked for `.cuaderno/`).\n\
             Hint: run `cdno init {}` to scaffold one, or set \
             `CUADERNO_VAULT_PATH` to point at an existing vault.",
            root.display(),
            root.display(),
        );
    }

    let config = VaultConfig::load(root)
        .with_context(|| format!("loading {}", root.join(paths::CONFIG_FILE).display()))?;
    let ignore = Arc::new(config.ignore_set().context("compiling the ignore set")?);
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> = Arc::new(
        SqliteIndex::open(root.join(paths::INDEX_DB))
            .with_context(|| format!("opening {}", root.join(paths::INDEX_DB).display()))?,
    );
    let (vault, report) = Vault::new(store.clone(), index.clone(), config)
        .context("constructing vault and reconciling index")?;
    tracing::debug!(
        scanned = report.scanned,
        added = report.added,
        updated = report.updated,
        removed = report.removed,
        errors = report.errors.len(),
        "startup reconciliation complete",
    );
    Ok(OpenedVault {
        vault,
        store,
        index,
        ignore,
        report,
    })
}
