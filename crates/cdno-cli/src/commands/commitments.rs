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
use crate::commands::orient::commitment_cells;

/// Render the commitments timeline for the vault at `root` as of
/// `today`, looking `weeks` weeks ahead (plus the standing overdue
/// look-back).
pub fn run(root: &Path, today: NaiveDate, weeks: u32, json: bool) -> Result<()> {
    if json {
        let (vault, _report) = bootstrap::open_vault(root)?;
        let lookahead_days = i64::from(weeks) * 7;
        let entries = vault
            .commitments(today, lookahead_days)
            .context("aggregating commitments")?;
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        print!("{}", build_commitments(root, today, weeks)?);
    }
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
    let header = format!("Commitments (next {weeks} week{suffix}, plus overdue)\n\n");
    if entries.is_empty() {
        return format!("{header}  (nothing due)\n");
    }
    // date / title / source columns; date and source stay whole, the
    // title reflows. Shared cell layout with `cdno orient` (#153).
    let mut table = crate::output::styled_table();
    for entry in entries {
        table.add_row(commitment_cells(entry));
    }
    crate::output::no_wrap_columns(&mut table, &[0, 2]);
    format!("{header}{}\n", crate::output::render(&table))
}
