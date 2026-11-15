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
//!
//! The database is wherever [`bootstrap::index_location`] resolves it —
//! `CUADERNO_INDEX_PATH` when set (#998) — so the file deleted here is
//! the same one the re-open creates. An overridden file is deleted only
//! when it is recognisably an index, and since the rebuilt file binds to
//! this vault, `reindex` is the one way to rebind an overridden index
//! that belonged to another vault; it says so when it does.

use std::path::Path;

use anyhow::{Context, Result};
use cdno_core::index::{SqliteIndex, canonical_vault_root};
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

    // Resolve before deleting anything: a bad override is refused with
    // the index untouched.
    let location = bootstrap::index_location(root)?;
    let rebound_from = if location.overridden {
        // Never delete a file that is not an index (a mistyped path).
        SqliteIndex::ensure_discardable(&location.path)?;
        SqliteIndex::bound_vault_root(&location.path)
            .unwrap_or(None)
            .filter(|bound| *bound != canonical_vault_root(root))
    } else {
        None
    };

    // Drop the cache, then re-open: `open_vault` reconciles the fresh
    // (empty) index against the filesystem, rebuilding every row.
    SqliteIndex::remove(&location.path);
    let (_vault, report) = bootstrap::open_vault(root).context("rebuilding the index")?;

    println!(
        "Reindexed {} note(s) ({} scanned).",
        report.added, report.scanned
    );
    // An override is invisible otherwise: name the file it resolved to.
    if location.overridden {
        println!(
            "Index: {} (from CUADERNO_INDEX_PATH).",
            location.path.display()
        );
    }
    if let Some(previous) = rebound_from {
        println!("Rebound the index from the vault at {previous} to this vault.");
    }
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
    // Artefacts are excluded by design, not by configuration, but the
    // count is reported for the same reason: a file absent from the index
    // is absent from search and lint, so it should never be a surprise.
    if report.artefacts > 0 {
        println!(
            "{} markdown file(s) skipped as attachment artefacts (filed documents, reachable through their evidence stub).",
            report.artefacts
        );
    }
    Ok(())
}
