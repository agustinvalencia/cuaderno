//! `cdno normalise [--check]` — reorder note frontmatter into the
//! canonical per-type key order (#233).
//!
//! Notes cdno creates are already canonical (their templates define the
//! order), so a clean vault is a no-op; this is for hand-authored or
//! migrated notes that have drifted. `--check` reports the out-of-order
//! notes without writing and exits non-zero (handy in CI / pre-commit);
//! the default rewrites them.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::bootstrap;

pub fn run(root: &Path, check: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let report = vault
        .normalise_notes(check)
        .context("normalising frontmatter")?;

    for (path, err) in &report.errors {
        eprintln!("  could not read {path}: {err}");
    }
    if report.skipped > 0 {
        eprintln!(
            "  skipped {} note(s) with an unknown type (run `cdno lint`)",
            report.skipped
        );
    }

    if report.changed.is_empty() {
        println!(
            "All {} note(s) already in canonical frontmatter order.",
            report.checked
        );
        return Ok(());
    }

    if check {
        println!(
            "{} note(s) out of canonical frontmatter order:",
            report.changed.len()
        );
        for path in &report.changed {
            println!("  {path}");
        }
        bail!("{} note(s) need `cdno normalise`", report.changed.len());
    }

    println!(
        "Normalised frontmatter in {} note(s):",
        report.changed.len()
    );
    for path in &report.changed {
        println!("  {path}");
    }
    Ok(())
}
