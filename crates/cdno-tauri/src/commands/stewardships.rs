//! Stewardship views (M7, plan §1.7; #59): the list behind
//! `/stewardships`, the composed detail behind `/stewardships/:slug`,
//! the tracking-template field discovery the log form needs, and the
//! one write (`log_tracking_entry`).
//!
//! Detail is a *status* surface, never a goal tracker. The trend
//! charts the frontend draws from `series` are read-only
//! visualisations — no targets, no red zones (that discipline lives in
//! the UI; the backend just hands over the numeric series
//! `tracking_series` already computes).

use std::collections::HashMap;

use chrono::{Local, NaiveDate, NaiveDateTime};

use cdno_domain::vault::{StewardshipSummary, StewardshipVariant, TrackingSeries};
use cdno_domain::{Context, Vault};

use crate::error::CmdError;
use crate::events::VaultArea;
use crate::state::AppState;
use crate::with_vault::with_vault;

use super::actions::record_and_emit;

/// How many most-recent tracking entries the detail view previews.
const RECENT_LIMIT: usize = 5;

/// The composed Stewardship Detail view-model (plan §1.7). One invoke
/// backs the whole `/stewardships/:slug` page: the dashboard body, the
/// trend series (empty for a flat stewardship — no `tracking/` subdir,
/// so the frontend omits the charts pane), and the last-few tracking
/// entries with a body excerpt for inline preview.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct StewardshipDetail {
    pub slug: String,
    /// The dashboard's body H1 (empty when absent — lint flags that
    /// separately). The frontend falls back to the slug for display.
    pub name: String,
    pub context: Context,
    pub variant: StewardshipVariant,
    /// The dashboard markdown below the frontmatter, rendered verbatim
    /// (status, habits, periodic commitments — the qualitative surface
    /// that the charts never replace).
    pub body_markdown: String,
    /// Numeric trend series, one per `(activity, table column)` pair
    /// that ever carries a number. Empty for a flat stewardship — the
    /// frontend shows the charts pane only when this is non-empty.
    pub series: Vec<TrackingSeries>,
    /// The most-recent tracking entries (up to five, newest first),
    /// flattened from the domain `TrackingEntry` (whose `VaultPath`
    /// can't carry a ts-rs derive) into a wire-ready shape.
    pub recent: Vec<TrackingEntryView>,
    /// Total tracking notes filed under this stewardship — the honest
    /// count behind the "N tracked" line, not just the previewed five.
    pub tracking_count: usize,
}

/// One tracking note in the detail's Recent list — the domain
/// `TrackingEntry` with its `VaultPath` lowered to a wire string so
/// the frontend can open the note in the reader and render its date,
/// activity, and excerpt.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct TrackingEntryView {
    pub path: String,
    pub stewardship: String,
    pub activity: String,
    pub date: NaiveDate,
    pub duration_min: Option<u32>,
    /// Raw wikilink string when the note pins a routine, else `None`.
    pub routine: Option<String>,
    /// First non-blank body line, capped for preview.
    pub body_excerpt: String,
}

/// One prompted field the tracking log form must gather for a given
/// activity — the `[variables.prompt]` names the resolved tracking
/// template references, with each prompt's message as the field label.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct TemplateField {
    pub name: String,
    /// The prompt message from `config.toml`'s `[variables.prompt]` —
    /// the human label the form shows above the input.
    pub prompt: String,
}

/// One [`StewardshipSummary`] per indexed stewardship, sorted by slug,
/// stamped as of `today` for the staleness line. Public and
/// synchronous — the test seam.
pub fn list_stewardships_impl(
    vault: &Vault,
    today: NaiveDate,
) -> Result<Vec<StewardshipSummary>, CmdError> {
    Ok(vault.list_stewardships(today)?)
}

/// Compose the Stewardship Detail bundle. Public and synchronous — the
/// test seam, exercised directly over the Memory doubles.
///
/// `recent` and `tracking_count` come from a single wide-window
/// `list_tracking` scan (already sorted most-recent-first): the count
/// is the full result length, the preview its first five. `series` is
/// only computed for an expanded stewardship — a flat dashboard has no
/// `tracking/` subdir, so there is nothing to chart.
pub fn get_stewardship_detail_impl(
    vault: &Vault,
    slug: &str,
) -> Result<StewardshipDetail, CmdError> {
    let (fm, body, variant) = vault.get_stewardship(slug)?;

    // All-time window so `recent` and `tracking_count` see every entry
    // regardless of date (a future-dated typo still counts). MIN..MAX
    // is inclusive on both ends in `list_tracking`.
    let all = vault.list_tracking(slug, None, NaiveDate::MIN, NaiveDate::MAX)?;
    let tracking_count = all.len();
    let recent = all
        .into_iter()
        .take(RECENT_LIMIT)
        .map(|e| TrackingEntryView {
            path: e.path.to_string(),
            stewardship: e.stewardship,
            activity: e.activity,
            date: e.date,
            duration_min: e.duration_min,
            routine: e.routine,
            body_excerpt: e.body_excerpt,
        })
        .collect();

    // Charts are an expanded-only surface: skip the tracking-note scan
    // entirely for a flat stewardship (it would return empty anyway).
    let series = match variant {
        StewardshipVariant::Expanded => vault.tracking_series(slug)?,
        StewardshipVariant::Flat => Vec::new(),
    };

    Ok(StewardshipDetail {
        slug: slug.to_owned(),
        name: extract_h1(&body),
        context: fm.context,
        variant,
        body_markdown: body,
        series,
        recent,
        tracking_count,
    })
}

/// The prompted fields the tracking log form needs for `activity`.
/// Public and synchronous — the test seam.
///
/// The activity slug selects the template exactly as
/// `add_tracking_entry` does: `template_prompts("tracking", <slug>)`
/// resolves `tracking-<slug>` and falls back to the generic `tracking`
/// template, so the fields the form gathers match what the later
/// create call will enforce. The generic template carries no prompts,
/// so an activity with no custom template yields an empty list.
pub fn get_tracking_template_fields_impl(
    vault: &Vault,
    activity: &str,
) -> Result<Vec<TemplateField>, CmdError> {
    // `slugify` is total — it returns `"untitled"` for input with no
    // alphanumerics, never an empty string — so the slug is always a
    // resolvable variant. `template_prompts` looks up `tracking-<slug>`
    // and falls back to the generic template when it's absent.
    let slug = cdno_domain::slugify(activity);
    let prompts = vault.template_prompts("tracking", Some(slug.as_str()))?;
    Ok(prompts
        .into_iter()
        .map(|(name, prompt)| TemplateField { name, prompt })
        .collect())
}

/// First `# ` heading in `body`, or an empty string. Mirrors the
/// domain's `extract_h1` (which is crate-private); a stewardship
/// dashboard always leads with its name as an H1.
fn extract_h1(body: &str) -> String {
    body.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|h| h.trim().to_owned())
        .unwrap_or_default()
}

/// Every indexed stewardship with its staleness line — the list behind
/// `/stewardships`. Pure read: no journal, no events.
#[tauri::command]
pub async fn list_stewardships(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<StewardshipSummary>, CmdError> {
    let today = Local::now().date_naive();
    with_vault(&state.vault, move |vault| {
        list_stewardships_impl(vault, today)
    })
    .await?
}

/// The composed Stewardship Detail read behind `/stewardships/:slug`.
#[tauri::command]
pub async fn get_stewardship_detail(
    state: tauri::State<'_, AppState>,
    slug: String,
) -> Result<StewardshipDetail, CmdError> {
    with_vault(&state.vault, move |vault| {
        get_stewardship_detail_impl(vault, &slug)
    })
    .await?
}

/// The tracking-template fields the log form should render for
/// `activity` (debounced on the typed activity in the UI). Pure read.
#[tauri::command]
pub async fn get_tracking_template_fields(
    state: tauri::State<'_, AppState>,
    activity: String,
) -> Result<Vec<TemplateField>, CmdError> {
    with_vault(&state.vault, move |vault| {
        get_tracking_template_fields_impl(vault, &activity)
    })
    .await?
}

/// File one tracking note under an expanded stewardship — the detail
/// view's Log Entry form. `vars` carries the prompted-field values the
/// form gathered from [`get_tracking_template_fields`].
///
/// Unlike the project writes, `add_tracking_entry_with_vars` does **not**
/// stage a daily-log line: the only file it touches is the new tracking
/// note, so that single path is all we journal. Two user-fixable domain
/// errors surface here as `Invalid` (via `From<DomainError>`): filing on
/// a flat stewardship (`TrackingOnFlatStewardship`) and a same-day,
/// same-activity duplicate (`AlreadyExists`) — both carry a good message
/// the UI toasts verbatim.
#[tauri::command]
pub async fn log_tracking_entry<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    stewardship: String,
    activity: String,
    routine: Option<String>,
    content: String,
    vars: HashMap<String, String>,
) -> Result<(), CmdError> {
    let now: NaiveDateTime = Local::now().naive_local();
    let path = with_vault(&state.vault, move |vault| {
        vault
            .add_tracking_entry_with_vars(
                now,
                &stewardship,
                &activity,
                routine.as_deref(),
                &content,
                &vars,
            )
            .map(|(path, _source)| path)
    })
    .await??;
    record_and_emit(&app, &state, vec![path], vec![VaultArea::Stewardships]);
    Ok(())
}
