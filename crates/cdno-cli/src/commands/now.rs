//! `cdno now`: what you are in the middle of.
//!
//! A read verb over `Vault::current_focus`, which replays the `## Logs`
//! of today and the `[focus] carry_over_days` days before it rather than
//! holding state — so a start made from the CLI,
//! from an agent over MCP, or by hand in an editor all count, and a
//! completion, a drop or a pause clears it. Focus is one slot: a newer
//! start displaces an older one for good. Nothing to keep in sync.
//!
//! `action promote` rewrites the bullet it matched, and the focus
//! follows it: the `action promoted on` line promotion logs renames the
//! open start to the new note, keeping its start time, so the close
//! verbs still pair. The domain pins that in
//! `a_promotion_between_start_and_close_moves_the_focus_to_the_note`.
//!
//! A hand-written line has to be in the shape the writers emit:
//! `- **HH:MM**: started [[slug]] — text`, separated by an em dash
//! (U+2014). The domain's parser requires that codepoint exactly
//! (`parse_focus_marker`, cdno-domain `vault/context.rs`), so a line
//! typed with an ASCII hyphen is silently not a focus. That strictness
//! is deliberate — prose beginning "started something" must not
//! register — but it does mean "by hand" means "in that shape".
//!
//! A focus can be older than today: `current_focus` reads back a start
//! from an earlier day within `carry_over_days` (RFC 0005 §5.2), and a
//! `resumed` line re-anchors one with the original start as its
//! `origin`. So the elapsed time is computed between datetimes — the
//! focus's own date plus its stamp, against the clock — and a start from
//! yesterday afternoon reads `18h 55m` the next morning, never a time-of-day
//! difference. Three renderings follow (RFC 0005 §5.7): `since` for a start,
//! `picked up … (started …)` for a resumed focus, and `Nothing started.`
//! with the most recent open pause and its `next:` hint.
//!
//! `--line` is the same truth squeezed into one sanitised line of at most
//! 160 characters for prompt segments and hooks. It is the one mode that
//! never fails: no vault or an unreadable log prints nothing and exits 0,
//! because a status line that breaks the prompt it is embedded in is worse
//! than a missing one. It does not return instantly under contention: with
//! the write lock held elsewhere and a stale index, it waits out the 5 s
//! lock timeout and then prints. It wins over `--json` when both are given.
//!
//! Rendering is split from I/O the way `orient` and `status` split it:
//! [`build_now`] returns the text so tests assert on a string without
//! capturing stdout. [`run`] does not call it — it opens the vault,
//! branches on `--json`, and calls `render` itself.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Weekday};

use cdno_domain::{CurrentFocus, LastPause};

use crate::bootstrap;
use crate::output::sanitise;
use crate::output::style::{Palette, Role};

/// The longest `--line` may be, in characters, the ellipsis included.
const LINE_CAP: usize = 160;

/// The `--json` shape, built by hand rather than derived.
///
/// `CurrentFocus` is a domain type with no `Serialize`, and adding one
/// there to serve a CLI flag would push a wire concern into the domain
/// — the same reason the MCP DTOs live outside it. `cdno-cli` does not
/// depend on `serde` directly either, only `serde_json`.
///
/// Every key is always present (RFC 0005 §5.7): an absent focus
/// serialises as all-null rather than a bare `null`, so a caller can test
/// one field (`.project == null`) without first branching on the shape of
/// the document. `action` stays the raw bullet text, what
/// `complete_action` expects back; `title` is it made readable and `note`
/// the attached note's path or null. `started` (HH:MM) stays for
/// compatibility; `started_at` and `date` say which day. `carried` is
/// "the focus's date is not today", and `origin` is null unless the focus
/// was resumed. `last_paused` is the newest open pause across projects,
/// whether or not something is also started.
pub fn now_json(
    focus: Option<&CurrentFocus>,
    last_paused: Option<&LastPause>,
    now: NaiveDateTime,
) -> serde_json::Value {
    let last_paused = last_paused.map(|p| {
        serde_json::json!({
            "project": p.project,
            "action": p.action,
            "title": p.title(),
            "at": stamp(p.at),
            "next": p.next,
            "reason": p.reason,
        })
    });
    match focus {
        Some(f) => {
            let started_at = f.date.and_time(f.started);
            serde_json::json!({
                "project": f.project,
                "action": f.action,
                "title": f.title(),
                "note": f.note(),
                "energy": f.energy().map(|e| e.as_str()),
                "started": f.started.format("%H:%M").to_string(),
                "started_at": stamp(started_at),
                "date": f.date.format("%Y-%m-%d").to_string(),
                "carried": f.date != now.date(),
                "origin": f.origin.map(|o| serde_json::json!({ "started_at": stamp(o) })),
                "elapsed_minutes": elapsed_secs(started_at, now).map(|s| s / 60),
                "last_paused": last_paused,
            })
        }
        None => serde_json::json!({
            "project": null, "action": null, "title": null, "note": null,
            "energy": null, "started": null, "started_at": null, "date": null,
            "carried": null, "origin": null, "elapsed_minutes": null,
            "last_paused": last_paused,
        }),
    }
}

fn stamp(at: NaiveDateTime) -> String {
    at.format("%Y-%m-%dT%H:%M").to_string()
}

/// Print what is currently started, for the vault at `root` as of `now`.
pub fn run(root: &Path, now: NaiveDateTime, json: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let focus = vault
        .current_focus(now.date())
        .context("reading the current focus")?;
    let pauses = vault
        .open_pauses(now.date())
        .context("reading the open pauses")?;
    if json {
        let value = now_json(focus.as_ref(), pauses.latest.as_ref(), now);
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    print!("{}", render(focus.as_ref(), pauses.latest.as_ref(), now));
    Ok(())
}

/// Open the vault and render the current focus as a string. Split from
/// [`run`] so tests can assert on the text.
pub fn build_now(root: &Path, now: NaiveDateTime) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let focus = vault
        .current_focus(now.date())
        .context("reading the current focus")?;
    let pauses = vault
        .open_pauses(now.date())
        .context("reading the open pauses")?;
    Ok(render(focus.as_ref(), pauses.latest.as_ref(), now))
}

/// The `--line` text for the vault at `root`: one sanitised line capped
/// at 160 characters, no trailing newline. `main` prints nothing and
/// exits 0 when this errors, or when no vault is found at all.
pub fn build_line(root: &Path, now: NaiveDateTime) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let focus = vault.current_focus(now.date())?;
    let pauses = vault.open_pauses(now.date())?;
    Ok(line(focus.as_ref(), pauses.latest.as_ref(), now))
}

/// How long ago `started` was, in words. `None` whenever the stamp is
/// ahead of `now` — a clock moving backwards (a timezone change, an NTP
/// correction), or a line typed into the log with a stamp later than
/// now, which this module's own "by hand" route invites. Saying nothing
/// beats "-3h ago".
///
/// Both ends are datetimes: the caller builds `started` from the
/// focus's own date plus its stamp, so a focus carried over midnight
/// reads as the long time it is — 08:00 yesterday to 10:00 today is
/// `26h`, not `2h`.
///
/// `pub` so `tests/now.rs` can pin it directly: it is the one piece of
/// arithmetic here, and the crate's convention is a `pub` seam over an
/// inline `#[cfg(test)]` module.
pub fn elapsed_since(started: NaiveDateTime, now: NaiveDateTime) -> Option<String> {
    let mins = elapsed_secs(started, now)? / 60;
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

/// Seconds elapsed, `None` for a start ahead of `now`.
///
/// Compared in SECONDS before dividing: `now` carries seconds while the
/// log stamp is minute-granular, so a start 1..59 seconds ahead divides
/// to 0 minutes and would slip past a minutes guard to read "just now".
fn elapsed_secs(started: NaiveDateTime, now: NaiveDateTime) -> Option<i64> {
    let secs = (now - started).num_seconds();
    (secs >= 0).then_some(secs)
}

fn weekday_name(date: NaiveDate) -> &'static str {
    match date.weekday() {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

/// `HH:MM`, prefixed with the weekday when the day is not today, or with
/// the date once it is more than six days back (a weekday name would then
/// be ambiguous).
pub(crate) fn when(at: NaiveDateTime, today: NaiveDate) -> String {
    let time = at.format("%H:%M");
    if at.date() == today {
        time.to_string()
    } else if (today - at.date()).num_days() > 6 {
        format!("{} {time}", at.date().format("%Y-%m-%d"))
    } else {
        format!("{} {time}", weekday_name(at.date()))
    }
}

/// The three human renderings of RFC 0005 §5.7.
fn render(
    focus: Option<&CurrentFocus>,
    last_paused: Option<&LastPause>,
    now: NaiveDateTime,
) -> String {
    let palette = Palette::active();
    let today = now.date();
    let Some(focus) = focus else {
        let mut out = format!("{}\n", palette.paint(Role::Muted, "Nothing started."));
        if let Some(p) = last_paused {
            out.push_str(&format!(
                "{}\n",
                palette.paint(Role::Muted, &last_paused_text(p, today))
            ));
        }
        return out;
    };
    let started_at = focus.date.and_time(focus.started);
    let title = sanitise(&focus.title());
    let what = match focus.energy() {
        Some(e) => format!("{title} ({})", e.as_str()),
        None => title,
    };
    let clause = match focus.origin {
        Some(origin) => {
            let picked = if focus.date == today {
                format!("{} today", focus.started.format("%H:%M"))
            } else {
                when(started_at, today)
            };
            format!("picked up {picked} (started {})", when(origin, today))
        }
        None => {
            let since = format!("since {}", when(started_at, today));
            match elapsed_since(started_at, now) {
                Some(ago) => format!("{since} ({ago})"),
                None => since,
            }
        }
    };
    format!(
        "{} {} {}\n",
        palette.paint(Role::Meta, "On"),
        palette.paint(Role::Slug, &sanitise(&focus.project)),
        palette.paint(Role::Meta, &format!("\u{2014} {what}, {clause}.")),
    )
}

/// `Last paused: <project> — <title> (<when>), next: <hint>`.
fn last_paused_text(p: &LastPause, today: NaiveDate) -> String {
    let mut text = format!(
        "Last paused: {} \u{2014} {} ({})",
        sanitise(&p.project),
        sanitise(&p.title()),
        when(p.at, today)
    );
    if let Some(next) = &p.next {
        text.push_str(&format!(", next: {}", sanitise(next)));
    }
    text
}

/// The `--line` form (RFC 0005 §5.7): `Focus: <project> — <title> (since
/// <time>)`, `(picked up <time>)` for a resumed focus, or `Focus: none`
/// with the last pause and its `next:` hint in parentheses.
fn line(
    focus: Option<&CurrentFocus>,
    last_paused: Option<&LastPause>,
    now: NaiveDateTime,
) -> String {
    let today = now.date();
    let text = match (focus, last_paused) {
        (Some(f), _) => {
            let at = when(f.date.and_time(f.started), today);
            let since = match f.origin {
                Some(_) => format!("picked up {at}"),
                None => format!("since {at}"),
            };
            format!(
                "Focus: {} \u{2014} {} ({since})",
                sanitise(&f.project),
                sanitise(&f.title())
            )
        }
        (None, Some(p)) => {
            let mut inner = format!(
                "last paused: {} \u{2014} {}",
                sanitise(&p.project),
                sanitise(&p.title())
            );
            if let Some(next) = &p.next {
                inner.push_str(&format!(", next: {}", sanitise(next)));
            }
            format!("Focus: none ({inner})")
        }
        (None, None) => "Focus: none".to_owned(),
    };
    cap(&text)
}

/// Truncate to [`LINE_CAP`] characters, the last of them `…` when cut.
fn cap(text: &str) -> String {
    if text.chars().count() <= LINE_CAP {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(LINE_CAP - 1).collect();
    out.push('\u{2026}');
    out
}
