//! `cdno weekly`: show the weekly review/plan note for an ISO week.
//!
//! The weekly note (design §5.2) is written by the weekly-review and
//! weekly-planning skills via the MCP `upsert_weekly_section` tool; this
//! is the read window onto it from the terminal — the only CLI surface
//! for weekly content. Rendering is split from I/O (`build_weekly`
//! returns the text) so tests assert on it without capturing stdout,
//! matching the `cdno orient` / `cdno status` seam.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};

use cdno_domain::WeeklyNoteView;

use crate::bootstrap;

/// Print the weekly note for the ISO week containing `date` (defaults to
/// `today` when `date` is `None`).
pub fn run(root: &Path, today: NaiveDate, date: Option<NaiveDate>) -> Result<()> {
    print!("{}", build_weekly(root, today, date)?);
    Ok(())
}

/// Open the vault, read the week's note, and render it to a string.
/// Split from [`run`] so tests can assert on the text without capturing
/// stdout.
pub fn build_weekly(root: &Path, today: NaiveDate, date: Option<NaiveDate>) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let when = date.unwrap_or(today);
    let view = vault
        .read_weekly_note(when)
        .context("reading the weekly note")?;
    Ok(render(&view, when))
}

/// Render the weekly note's body, or a placeholder when the week has no
/// note yet. Public so tests can assert on the formatted text.
pub fn render(view: &WeeklyNoteView, when: NaiveDate) -> String {
    if !view.exists {
        let iso = when.iso_week();
        return format!(
            "No weekly note for {}-W{:02} yet.\n  \
             Start one with a weekly review or `weekly-planning`.\n",
            iso.year(),
            iso.week(),
        );
    }
    // Print the body, dropping the YAML frontmatter for a clean terminal
    // view — the `# Week N, YYYY` heading and the four sections carry
    // everything a human reads.
    strip_frontmatter(&view.markdown)
}

/// Drop a leading `---\n…\n---\n` YAML frontmatter block, returning the
/// body with leading blank lines trimmed. Markdown without frontmatter is
/// returned unchanged.
fn strip_frontmatter(markdown: &str) -> String {
    if let Some(rest) = markdown.strip_prefix("---\n")
        && let Some(end) = rest.find("\n---\n")
    {
        return rest[end + "\n---\n".len()..]
            .trim_start_matches('\n')
            .to_owned();
    }
    markdown.to_owned()
}
