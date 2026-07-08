//! `cdno monthly`: show the monthly review note for a calendar month.
//!
//! The monthly note (design §5.2) is written by the monthly-review
//! ritual via the MCP `upsert_monthly_section` tool; this is the read
//! window onto it from the terminal — the only CLI surface for monthly
//! content besides `cdno review monthly`. Rendering is split from I/O
//! (`build_monthly` returns the text) so tests assert on it without
//! capturing stdout, matching the `cdno weekly` seam.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDate;

use cdno_domain::MonthlyNoteView;

use crate::bootstrap;

/// Print the monthly note for the calendar month containing `date`
/// (defaults to `today` when `date` is `None`).
pub fn run(root: &Path, today: NaiveDate, date: Option<NaiveDate>) -> Result<()> {
    print!("{}", build_monthly(root, today, date)?);
    Ok(())
}

/// Open the vault, read the month's note, and render it to a string.
/// Split from [`run`] so tests can assert on the text without capturing
/// stdout.
pub fn build_monthly(root: &Path, today: NaiveDate, date: Option<NaiveDate>) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let when = date.unwrap_or(today);
    let view = vault
        .read_monthly_note(when)
        .context("reading the monthly note")?;
    Ok(render(&view, when))
}

/// Render the monthly note's body, or a placeholder when the month has no
/// note yet. Public so tests can assert on the formatted text.
pub fn render(view: &MonthlyNoteView, when: NaiveDate) -> String {
    if !view.exists {
        return format!(
            "No monthly note for {} yet.\n  \
             Start one with `cdno review monthly`.\n",
            when.format("%Y-%m"),
        );
    }
    // Print the body, dropping the YAML frontmatter for a clean terminal
    // view — the `# <Month> YYYY` heading and the sections carry
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
