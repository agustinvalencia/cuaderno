//! `cdno status`: a quick snapshot — a one-line count header followed
//! by each active project and its top next action. Leaner than
//! `cdno orient` (no commitments detail, no suggestion); the two share
//! `Vault::orientation_context` and the project-next formatter.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDate;

use cdno_domain::OrientationContext;

use crate::bootstrap;
use crate::commands::orient::project_next;

/// Print a quick status snapshot for the vault at `root` as of `today`.
pub fn run(root: &Path, today: NaiveDate, json: bool) -> Result<()> {
    if json {
        let (vault, _report) = bootstrap::open_vault(root)?;
        let ctx = vault
            .orientation_context(today)
            .context("building orientation context")?;
        println!("{}", serde_json::to_string_pretty(&ctx)?);
    } else {
        print!("{}", build_status(root, today)?);
    }
    Ok(())
}

/// Open the vault and render the snapshot to a string. Split from
/// [`run`] so tests can assert on the text without capturing stdout.
pub fn build_status(root: &Path, today: NaiveDate) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let ctx = vault
        .orientation_context(today)
        .context("building orientation context")?;
    Ok(render(&ctx))
}

fn render(ctx: &OrientationContext) -> String {
    let mut out = format!(
        "{} active project{}, {} commitment{} due soon\n\n",
        ctx.projects.len(),
        plural(ctx.projects.len()),
        ctx.commitments.len(),
        plural(ctx.commitments.len()),
    );

    if ctx.projects.is_empty() {
        out.push_str("  (no active projects)\n");
    } else {
        // slug column stays whole; the next-action reflows (#153).
        let mut table = crate::output::styled_table();
        for p in &ctx.projects {
            table.add_row(vec![p.slug.clone(), format!("next: {}", project_next(p))]);
        }
        crate::output::no_wrap_columns(&mut table, &[0]);
        out.push_str(&crate::output::render(&table));
        out.push('\n');
    }

    out
}

/// `""` for one, `"s"` otherwise — for pluralising the count header.
fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
