use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;

use crate::bootstrap;

/// Append a log entry to the daily note for `at`. Creates the note
/// with a minimal scaffold if it doesn't exist.
pub fn run(root: &Path, at: NaiveDateTime, message: &str) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let path = vault
        .log_to_daily_note(at, message)
        .context("appending log entry to daily note")?;
    println!("Logged to {path}");
    Ok(())
}
