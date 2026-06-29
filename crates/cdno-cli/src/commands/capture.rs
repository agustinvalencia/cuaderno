use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;

use crate::bootstrap;

/// Drop a quick note into `inbox/` and print the resulting path.
pub fn run(root: &Path, at: NaiveDateTime, text: &str, json: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let path = vault
        .capture_to_inbox(at, text)
        .context("writing capture to inbox")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Captured to {path}"))?;
    Ok(())
}
