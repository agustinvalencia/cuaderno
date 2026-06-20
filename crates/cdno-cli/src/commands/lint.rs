use std::path::Path;

use anyhow::{Result, bail};

use crate::bootstrap;

/// Validate every indexed note and print a report.
///
/// Exits non-zero (via the returned `Err`) when there are **errors**
/// (unknown type, missing required field, append-only / attachment
/// violations). Warnings (e.g. broken wikilinks) are non-fatal by
/// default — `--strict` (`strict = true`) promotes them to the failure
/// threshold for CI gates that want zero dangling links. Mirrors
/// `cargo clippy`'s warn-by-default / `-D warnings` split.
///
/// Issues go to stdout (one per line, grep-friendly, severity-tagged);
/// the failure summary lands on stderr through `anyhow`.
pub fn run(root: &Path, strict: bool) -> Result<()> {
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

    let errors = report.error_count();
    let warnings = report.warning_count();

    // Errors always fail; warnings fail only under --strict.
    if errors > 0 || (strict && warnings > 0) {
        bail!("found {errors} error(s), {warnings} warning(s)");
    }

    // Warnings only, not strict: surface them but succeed.
    println!(
        "found {errors} error(s), {warnings} warning(s) (warnings are non-fatal; use --strict to fail)"
    );
    Ok(())
}
