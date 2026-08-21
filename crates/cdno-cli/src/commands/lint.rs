use std::path::Path;

use anyhow::{Result, bail};

use cdno_domain::lint::LintSeverity;

use crate::bootstrap;
use crate::output::style::Role;

/// Validate every indexed note and print a report.
///
/// Exits non-zero (via the returned `Err`) when there are **errors**
/// (unknown type, missing required field, append-only / attachment
/// violations). Warnings (e.g. broken wikilinks) are non-fatal by
/// default — `--strict` (`strict = true`) promotes them to the failure
/// threshold for CI gates that want zero dangling links. Mirrors
/// `cargo clippy`'s warn-by-default / `-D warnings` split.
///
/// Issues go to stdout (one per line, grep-friendly, severity-tagged).
/// A failure summary lands on stderr through `anyhow`; a non-fatal
/// warnings-only summary goes to stdout.
pub fn run(root: &Path, strict: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let report = vault.lint_all_notes()?;

    if report.is_clean() {
        println!("No issues found.");
        return Ok(());
    }

    // One issue per line, grep-friendly, exactly as before — the literal
    // text is unchanged and only the severity tag and path are painted.
    // That contract survives colour for free: stdout is not a terminal
    // precisely when the output is being piped into `grep`, which is
    // when the gate turns painting off.
    let palette = crate::output::style::Palette::active();
    for issue in &report.issues {
        let role = severity_role(issue.severity);
        println!(
            "{} {}: {}",
            palette.paint(role, &format!("[{}]", issue.severity.as_str())),
            palette.paint(Role::Meta, &issue.path.to_string()),
            crate::output::sanitise(&issue.message)
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

/// The style a lint severity reads in.
///
/// A named function rather than an inline match so the mapping can be
/// asserted: inverting it — errors rendered as warnings and warnings as
/// errors — is invisible to a test that only reads the literal
/// `[error]` / `[warning]` text, which is every test this command has.
pub fn severity_role(severity: LintSeverity) -> Role {
    match severity {
        LintSeverity::Error => Role::Error,
        LintSeverity::Warning => Role::Warn,
    }
}
