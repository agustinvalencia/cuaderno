use std::path::Path;

use anyhow::{Result, bail};

use crate::bootstrap;

/// Validate every indexed note and print a report.
///
/// Exits non-zero (via the returned `Err`) when any issues are found,
/// so the command composes with shell scripts and CI gates.
/// Issues go to stdout (one per line, grep-friendly); the error
/// summary lands on stderr through `anyhow`.
pub fn run(root: &Path) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let report = vault.lint_all_notes()?;

    if report.is_clean() {
        println!("No issues found.");
        return Ok(());
    }

    for issue in &report.issues {
        println!("{}: {}", issue.path, issue.message);
    }
    bail!("found {} lint issue(s)", report.issues.len());
}
