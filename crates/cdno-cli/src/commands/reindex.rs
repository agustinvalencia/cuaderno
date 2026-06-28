//! `cdno reindex` — rebuild the SQLite index from the markdown source
//! of truth.
//!
//! The index is a cache: every fact in it is derived from the notes on
//! disk, so dropping and rebuilding it is always safe. This is the
//! explicit recovery path for a stale or corrupt index (a corrupt index
//! also self-heals on open, but `reindex` forces a clean rebuild on
//! demand). The rebuild is just "delete the db, then open the vault" —
//! `open_vault` runs reconciliation, which repopulates the empty index
//! from every note.

use std::path::Path;

use anyhow::{Context, Result};
use cdno_core::index::SqliteIndex;
use cdno_core::paths;

use crate::bootstrap;

pub fn run(root: &Path) -> Result<()> {
    // Confirm we're actually in a vault before deleting anything.
    let cuaderno_dir = root.join(paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        anyhow::bail!(
            "no Cuaderno vault at {}; run `cdno init` to create one.",
            root.display()
        );
    }

    // Drop the cache, then re-open: `open_vault` reconciles the fresh
    // (empty) index against the filesystem, rebuilding every row.
    SqliteIndex::remove(root.join(paths::INDEX_DB));
    let (_vault, report) = bootstrap::open_vault(root).context("rebuilding the index")?;

    println!(
        "Reindexed {} note(s) ({} scanned).",
        report.added, report.scanned
    );
    // Make ignore-glob exclusions visible: a stray `**` that drops every
    // note from search/lint should never be silent. The files are on
    // disk untouched — removing the offending pattern and reindexing
    // brings them all back.
    if report.ignored > 0 {
        println!(
            "{} file(s) excluded by `ignore` globs (still on disk; clear the glob and reindex to restore).",
            report.ignored
        );
    }
    Ok(())
}
