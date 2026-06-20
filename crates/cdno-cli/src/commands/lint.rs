use std::path::Path;

use anyhow::{Result, bail};
use cdno_domain::LintSeverity;

use crate::bootstrap;

/// Validate every indexed note and print a report.
///
/// Exits non-zero (via the returned `Err`) when any issues are found,
/// so the command composes with shell scripts and CI gates.
/// Issues go to stdout (one per line, grep-friendly, severity-tagged);
/// the error summary lands on stderr through `anyhow`.
pub fn run(root: &Path) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let report = vault.lint_all_notes()?;

    if report.is_clean() {
        println!("No issues found.");
        return Ok(());
    }

    for issue in &report.issues {
        println!(
            "[{}] {}: {}",
            issue.severity.as_str(),
            issue.path,
            issue.message
        );
    }

    let errors = report
        .issues
        .iter()
        .filter(|i| i.severity == LintSeverity::Error)
        .count();
    let warnings = report.issues.len() - errors;
    bail!("found {errors} error(s), {warnings} warning(s)");
}
