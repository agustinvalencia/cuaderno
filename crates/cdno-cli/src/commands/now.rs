//! `cdno now`: what you are in the middle of.
//!
//! A read verb over [`Vault::current_focus`], which replays today's
//! `## Logs` rather than holding state — so a start made from the CLI,
//! from an agent over MCP, or by hand in an editor all count, and a
//! completion or a drop clears it. Nothing to keep in sync.
//!
//! Rendering is split from I/O the way `orient` and `status` split it:
//! [`build_now`] returns the text so tests assert on a string without
//! capturing stdout, and [`run`] prints what it returns.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveTime, Timelike};

use cdno_domain::CurrentFocus;

use crate::bootstrap;
use crate::output::sanitise;
use crate::output::style::{Palette, Role};

/// The `--json` shape, built by hand rather than derived.
///
/// `CurrentFocus` is a domain type with no `Serialize`, and adding one
/// there to serve a CLI flag would push a wire concern into the domain
/// — the same reason the MCP DTOs live outside it. `cdno-cli` does not
/// depend on `serde` directly either, only `serde_json`, so a local
/// derive would mean a new dependency for three fields.
///
/// An absent focus serialises as all-null rather than a bare `null`, so
/// a caller can test one field (`.project == null`) without first
/// branching on the shape of the document.
pub fn now_json(focus: Option<&CurrentFocus>) -> serde_json::Value {
    match focus {
        Some(f) => serde_json::json!({
            "project": f.project,
            "action": f.action,
            "started": f.started.format("%H:%M").to_string(),
        }),
        None => serde_json::json!({
            "project": null, "action": null, "started": null,
        }),
    }
}

/// Print what is currently started, for the vault at `root` as of `today`.
pub fn run(root: &Path, today: NaiveDate, now: NaiveTime, json: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let focus = vault
        .current_focus(today)
        .context("reading the current focus")?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&now_json(focus.as_ref()))?
        );
        return Ok(());
    }
    print!("{}", render(focus.as_ref(), now));
    Ok(())
}

/// Open the vault and render the current focus as a string. Split from
/// [`run`] so tests can assert on the text.
pub fn build_now(root: &Path, today: NaiveDate, now: NaiveTime) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let focus = vault
        .current_focus(today)
        .context("reading the current focus")?;
    Ok(render(focus.as_ref(), now))
}

/// How long ago `started` was, in words. `None` when the start is in
/// the future, which happens when the clock moved — a nap past
/// midnight, a timezone change. Saying nothing beats "-3h ago".
///
/// `pub` so `tests/now.rs` can pin it directly: it is the one piece of
/// arithmetic here, and the crate's convention is a `pub` seam over an
/// inline `#[cfg(test)]` module.
pub fn elapsed_since(started: NaiveTime, now: NaiveTime) -> Option<String> {
    // Compare in SECONDS before dividing. `now` carries seconds (it is
    // `Local::now().time()`) while the log stamp is minute-granular, so
    // a start 1..59 seconds ahead divides to 0 and would slip past a
    // `mins < 0` guard to read "just now" — the doc above promises None
    // for any future start, and truncation toward zero would quietly
    // break that for the whole first minute.
    let secs = now.num_seconds_from_midnight() as i64 - started.num_seconds_from_midnight() as i64;
    if secs < 0 {
        return None;
    }
    let mins = secs / 60;
    if mins < 1 {
        return Some("just now".to_owned());
    }
    if mins < 60 {
        return Some(format!("{mins}m"));
    }
    let (h, m) = (mins / 60, mins % 60);
    Some(if m == 0 {
        format!("{h}h")
    } else {
        format!("{h}h {m}m")
    })
}

fn render(focus: Option<&CurrentFocus>, now: NaiveTime) -> String {
    let palette = Palette::active();
    let Some(focus) = focus else {
        // Not an empty frame: with nothing open, the honest answer is
        // the prompt to pick one.
        return format!(
            "{}\n",
            palette.paint(
                Role::Muted,
                "Nothing started yet. `cdno orient` suggests one thing to begin."
            )
        );
    };
    let since = match elapsed_since(focus.started, now) {
        Some(ago) => format!("since {} · {ago}", focus.started.format("%H:%M")),
        None => format!("since {}", focus.started.format("%H:%M")),
    };
    format!(
        "{} {}\n  {}\n",
        palette.paint(Role::Slug, &sanitise(&focus.project)),
        palette.paint(Role::Meta, &since),
        palette.paint(Role::Prose, &sanitise(&focus.action)),
    )
}
