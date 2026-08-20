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
use crate::output::style::{Role, cell};

/// Print a quick status snapshot for the vault at `root` as of `today`.
pub fn run(root: &Path, today: NaiveDate, no_interactive: bool, json: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let ctx = vault
        .orientation_context(today)
        .context("building orientation context")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&ctx)?);
        return Ok(());
    }
    print!("{}", render(&ctx));
    // `--json` implies non-interactive: a prompt writes to stdout, which
    // would corrupt the result a scripted caller is parsing.
    let interactive = crate::prompt::is_interactive(no_interactive || json);
    crate::prompt::drill_down(
        &ctx.projects,
        "Inspect a project",
        interactive,
        |p| format!("{} ({})", p.slug, p.context.as_str()),
        |p| {
            print!("{}", crate::commands::project::render_show(p));
            Ok(())
        },
    )
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
    let palette = crate::output::style::Palette::active();
    // Deliberately still a table, not cards. `status` is the lean
    // snapshot next to `orient`'s fuller view; giving each project three
    // gutter lines here would make the quick check taller than the
    // considered one.
    let mut out = format!(
        "{}\n\n",
        palette.paint(
            Role::Heading,
            &format!(
                "{} active project{}, {} commitment{} due soon",
                ctx.projects.len(),
                plural(ctx.projects.len()),
                ctx.commitments.len(),
                plural(ctx.commitments.len()),
            )
        )
    );

    if ctx.projects.is_empty() {
        out.push_str(&format!(
            "  {}\n",
            palette.paint(Role::Muted, "(no active projects)")
        ));
    } else {
        // slug column stays whole; the next-action reflows (#153).
        let mut table = crate::output::styled_table();
        for p in &ctx.projects {
            table.add_row(vec![
                cell(Role::Slug, &p.slug),
                cell(Role::Meta, format!("next: {}", project_next(p))),
            ]);
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
