//! Shared CLI output formatting (#153).
//!
//! One place that presets how `cdno` renders tabular output, so every
//! list-style command looks the same and the house style is tunable
//! here rather than re-derived per command.
//!
//! Why this lives in `cdno-cli` and not the domain: the domain returns
//! plain data; presentation is the CLI's job. We deliberately do *not*
//! derive presentation traits on `cdno-domain` types — that would leak a
//! formatting dependency into the layer the MCP server and Tauri app also
//! depend on. `comfy-table`'s runtime API keeps the coupling here, in the
//! binary. (The MCP stdout channel is JSON-RPC and never touches this
//! module — table code is only ever on the CLI path.)

use std::io::IsTerminal;

use comfy_table::{ColumnConstraint, ContentArrangement, Table, presets};

/// Width assumed when stdout is not a terminal (piped, redirected, or a
/// test calling a `render` helper directly). Wide enough to keep most
/// rows on one line, narrow enough that genuinely long cells still wrap
/// instead of running off forever — and fixed, so piped/test output is
/// deterministic.
const NON_TTY_WIDTH: u16 = 100;

/// A borderless table preset to the cuaderno house style: no rules or
/// frame, dynamic column arrangement so long cells wrap to the available
/// width, and terminal-width auto-detection (falling back to a fixed
/// width off a tty). Subcommands add rows and render with `to_string()`.
pub fn styled_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(presets::NOTHING)
        .set_content_arrangement(ContentArrangement::Dynamic);
    // With the `tty` feature, comfy-table measures the terminal itself
    // when stdout is a tty. Off a tty it has nothing to measure and would
    // stop wrapping, so pin a width for the piped/redirected/test case.
    if !std::io::stdout().is_terminal() {
        table.set_width(NON_TTY_WIDTH);
    }
    table
}

/// Pin the given columns to their content width so they never wrap, and
/// the free-text column(s) absorb all the wrapping instead. Without this,
/// `ContentArrangement::Dynamic` balances width across every column and
/// will happily wrap a slug or a short badge mid-token once a third
/// column competes for space — identifiers should stay whole and only the
/// prose column should reflow. Call after the rows are added (columns
/// don't exist until then).
pub fn no_wrap_columns(table: &mut Table, columns: &[usize]) {
    for &index in columns {
        if let Some(column) = table.column_mut(index) {
            column.set_constraint(ColumnConstraint::ContentWidth);
        }
    }
}

/// Render a table to a string with trailing whitespace stripped from each
/// line. comfy-table pads every cell out to its column width, which on a
/// borderless table leaves a ragged trail of spaces running to the table
/// edge; trimming keeps the inter-column alignment but drops that trail,
/// so output is clean to read, copy, and diff. Prefer this over calling
/// `Table::to_string()` directly.
pub fn render(table: &Table) -> String {
    let rendered = table.to_string();
    let mut out = String::with_capacity(rendered.len());
    for (i, line) in rendered.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    out
}

/// Emit a write verb's result. With `json`, prints a `{path, message}`
/// object (the same shape as the MCP `WriteResultDto`) so scripted
/// callers get a stable, parseable result; otherwise prints the
/// human-readable `message` line (#227). Write verbs route their success
/// output through here instead of `println!` so `--json` isn't a silent
/// no-op on them.
pub fn emit_write_result(json: bool, path: &str, message: &str) -> anyhow::Result<()> {
    if json {
        let payload = serde_json::json!({ "path": path, "message": message });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{message}");
    }
    Ok(())
}
