//! `cdno commitments [--weeks N]`: the four-source aggregated timeline
//! (`Vault::commitments`), formatted for the terminal. Shares the
//! per-entry formatting with `cdno orient` so the same row prints the
//! same way in both surfaces.
//!
//! `--weeks` defaults to 2 — short enough for a weekly review, long
//! enough to surface near-future commitments without scrolling. The
//! standing 30-day overdue look-back from `Vault::commitments` always
//! applies on top.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDate;

use crate::bootstrap;
use crate::commands::orient::commitment_line;

/// Render the commitments timeline for the vault at `root` as of
/// `today`, looking `weeks` weeks ahead (plus the standing overdue
/// look-back).
pub fn run(root: &Path, today: NaiveDate, weeks: u32) -> Result<()> {
    print!("{}", build_commitments(root, today, weeks)?);
    Ok(())
}

/// Open the vault, query commitments, and render to a string. Split
/// from [`run`] so tests can assert on the text without capturing
/// stdout — mirrors orient's `build_orientation` seam.
pub fn build_commitments(root: &Path, today: NaiveDate, weeks: u32) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let lookahead_days = i64::from(weeks) * 7;
    let entries = vault
        .commitments(today, lookahead_days)
        .context("aggregating commitments")?;
    Ok(render(&entries, weeks))
}

fn render(entries: &[cdno_domain::CommitmentEntry], weeks: u32) -> String {
    let suffix = if weeks == 1 { "" } else { "s" };
    let mut out = format!("Commitments (next {weeks} week{suffix}, plus overdue)\n\n");
    if entries.is_empty() {
        out.push_str("  (nothing due)\n");
    } else {
        for entry in entries {
            out.push_str(&format!("  {}\n", commitment_line(entry)));
        }
    }
    out
}
