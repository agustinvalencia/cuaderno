//! Context-gathering queries used by the MCP `get_*_context` tools
//! (and any future skill / UI consumer that wants the same shapes).
//!
//! These are the domain primitives that GH #142 calls for: each one
//! returns a typed slice of vault state for a specific window or
//! relationship. The MCP handlers in `cdno-mcp` compose them; the
//! CLI does not (the CLI's `cdno orient` / `cdno status` already
//! have their own composition surface).
//!
//! Eight methods land here:
//!
//! - [`Vault::weekly_logs`] — flat log lines from every daily note
//!   in the ISO week containing `week_of`.
//! - [`Vault::completed_actions_between`] — action notes with
//!   `status: completed` and `completed:` in `[from, to]`.
//! - [`Vault::project_state_changes_between`] — `was → now` entries
//!   parsed from daily-note `## Logs`.
//! - [`Vault::stuck_projects`] — active projects whose project map
//!   hasn't been modified in `unchanged_for_days` days.
//! - [`Vault::get_project_full`] — typed frontmatter + raw body of a
//!   project map.
//! - [`Vault::daily_log_mentions`] — log lines that wikilink the
//!   project, across daily notes since `since`.
//! - [`Vault::project_backlinks`] — backlinks grouped by note type.
//! - [`Vault::list_tracking`] — tracking notes for a stewardship,
//!   optionally filtered by activity and a date window.
//! - [`Vault::tracking_series`] — numeric time series lifted from the
//!   tracking notes' tables, ready for trend charts.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime};

use std::collections::BTreeMap;

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::markdown::{MarkdownDocument, extract_first_table};
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{
    ActionFrontmatter, ActionStatus, ProjectFrontmatter, ProjectStatus, TrackingFrontmatter,
};
use crate::note_type::NoteType;

use super::DAILY_LOGS_SECTION;
use super::Vault;
use super::projects::ProjectSummary;

// ---------------------------------------------------------------------
// Return types
// ---------------------------------------------------------------------

/// One log line pulled from a daily note's `## Logs` section. The
/// `text` field collapses any indented continuation lines into a
/// single-line summary (separated by `; `) so downstream renderers
/// don't have to handle multi-line entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyLogLine {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub text: String,
}

/// One completed action note in the `[from, to]` window. Carries
/// just enough for a wins-list renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedActionEntry {
    pub slug: String,
    pub project: String,
    pub title: String,
    pub completed: NaiveDate,
    pub path: VaultPath,
}

/// One `was → now` project-state change parsed from a daily note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectStateChange {
    pub date: NaiveDate,
    pub project: String,
    pub old_state: String,
    pub new_state: String,
}

/// Backlinks to a project, grouped by source note type so consumers
/// can render "linked portfolios" vs "linked questions" sections
/// without per-row type lookups.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectBacklinks {
    pub portfolios: Vec<VaultPath>,
    pub questions: Vec<VaultPath>,
    pub evidence: Vec<VaultPath>,
    pub actions: Vec<VaultPath>,
    /// Anything else (commitments, daily notes, hand-edited
    /// references). Lets the caller still render every backlink even
    /// when the source type isn't one of the call-out groups.
    pub other: Vec<VaultPath>,
}

/// One numeric time series lifted from a stewardship's tracking
/// notes, ready for a trend chart. See [`Vault::tracking_series`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TrackingSeries {
    /// `"{activity} · {column header}"` — e.g. `"gym · Weight (kg)"`.
    pub name: String,
    /// One point per tracking note that had a numeric value in the
    /// column, sorted by date.
    pub points: Vec<TrackingPoint>,
}

/// One dated value in a [`TrackingSeries`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TrackingPoint {
    pub date: NaiveDate,
    pub value: f64,
}

/// One tracking note in `list_tracking` output, with a short body
/// excerpt so a consumer can preview without fetching the full file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TrackingEntry {
    pub path: VaultPath,
    pub stewardship: String,
    pub activity: String,
    pub date: NaiveDate,
    pub duration_min: Option<u32>,
    /// Raw wikilink string when present (e.g.
    /// `"[[stewardships/health/routines/upper-body-a]]"`).
    pub routine: Option<String>,
    /// First non-blank line of the body (after the H1) — capped at
    /// 200 chars so the output stays bounded.
    pub body_excerpt: String,
}

impl Vault {
    // -----------------------------------------------------------------
    // weekly_logs
    // -----------------------------------------------------------------

    /// Every log line from every daily note in the ISO week
    /// containing `week_of` (Monday-to-Sunday, locale-independent).
    /// Missing daily notes are skipped silently; a malformed one
    /// surfaces its parse error.
    pub fn weekly_logs(&self, week_of: NaiveDate) -> Result<Vec<DailyLogLine>, DomainError> {
        let monday = monday_of_iso_week(week_of);
        let mut out = Vec::new();
        for offset in 0..7 {
            let date = monday + Duration::days(offset);
            let path = VaultPath::new(cdno_core::paths::daily_note_relpath(date))?;
            if !self.store.exists(&path)? {
                continue;
            }
            let raw = self.store.read_file(&path)?;
            let doc = MarkdownDocument::parse(raw)?;
            let section = match doc.section(DAILY_LOGS_SECTION) {
                Ok(s) => s.to_owned(),
                Err(_) => continue, // tolerate a missing Logs section
            };
            for (time, text) in parse_log_lines(&section) {
                out.push(DailyLogLine { date, time, text });
            }
        }
        Ok(out)
    }

    // -----------------------------------------------------------------
    // completed_actions_between
    // -----------------------------------------------------------------

    /// Action notes with `status: completed` and a `completed:` date
    /// in the inclusive `[from, to]` window. Sorted oldest-first.
    pub fn completed_actions_between(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<CompletedActionEntry>, DomainError> {
        let entries = self.index.list_by_type(NoteType::Action.as_str())?;
        let mut out = Vec::new();
        for entry in entries {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, body) = Frontmatter::parse(&raw)?;
            let af = ActionFrontmatter::try_from(fm)?;
            if af.status != ActionStatus::Completed {
                continue;
            }
            let Some(completed) = af.completed else {
                continue;
            };
            if completed < from || completed > to {
                continue;
            }
            out.push(CompletedActionEntry {
                slug: path_stem(&entry.path),
                project: af.project,
                title: extract_h1(body).unwrap_or_else(|| path_stem(&entry.path)),
                completed,
                path: entry.path,
            });
        }
        out.sort_by(|a, b| a.completed.cmp(&b.completed).then(a.slug.cmp(&b.slug)));
        Ok(out)
    }

    // -----------------------------------------------------------------
    // project_state_changes_between
    // -----------------------------------------------------------------

    /// `was → now` project-state changes in `[from, to]`, parsed
    /// from daily-note `## Logs` sections. Recognises the canonical
    /// format that `Vault::update_project_state` writes:
    /// `state on [[<slug>]]` then indented `was:` / `now:` lines.
    pub fn project_state_changes_between(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<ProjectStateChange>, DomainError> {
        let mut out = Vec::new();
        let mut date = from;
        while date <= to {
            let path = VaultPath::new(cdno_core::paths::daily_note_relpath(date))?;
            if self.store.exists(&path)? {
                let raw = self.store.read_file(&path)?;
                let doc = MarkdownDocument::parse(raw)?;
                if let Ok(section) = doc.section(DAILY_LOGS_SECTION) {
                    for (project, was, now) in parse_state_changes(section) {
                        out.push(ProjectStateChange {
                            date,
                            project,
                            old_state: was,
                            new_state: now,
                        });
                    }
                }
            }
            date += Duration::days(1);
        }
        Ok(out)
    }

    // -----------------------------------------------------------------
    // stuck_projects
    // -----------------------------------------------------------------

    /// Active projects whose project map hasn't been modified in at
    /// least `unchanged_for_days` days. `mtime_ns` from the index is
    /// the source of truth — reconciliation keeps it in sync with
    /// the filesystem.
    pub fn stuck_projects(
        &self,
        today: NaiveDate,
        unchanged_for_days: i64,
    ) -> Result<Vec<ProjectSummary>, DomainError> {
        let threshold_ns = mtime_threshold_ns(today, unchanged_for_days);
        let entries = self.index.list_by_type(NoteType::Project.as_str())?;
        let mut out = Vec::new();
        for entry in entries {
            // Parked / completed projects are out of scope — the
            // "stuck" heuristic only makes sense for active work.
            // Cheap check: skip parked-folder paths, then read the
            // file to confirm frontmatter status.
            let raw = self.store.read_file(&entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let pf = ProjectFrontmatter::try_from(fm)?;
            if pf.status != ProjectStatus::Active {
                continue;
            }
            if entry.mtime_ns > threshold_ns {
                continue;
            }
            out.push(self.project_summary(&path_stem(&entry.path))?);
        }
        out.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(out)
    }

    // -----------------------------------------------------------------
    // get_project_full
    // -----------------------------------------------------------------

    /// The typed frontmatter and the raw body of a project map.
    /// Mirrors [`Vault::get_portfolio`](Self::get_portfolio) and
    /// [`Vault::get_stewardship`](Self::get_stewardship). Resolves
    /// the slug against both `projects/` and `projects/_parked/`.
    pub fn get_project_full(
        &self,
        slug: &str,
    ) -> Result<(ProjectFrontmatter, String), DomainError> {
        let active_path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))?;
        let parked_path =
            VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))?;
        let path = if self.store.exists(&active_path)? {
            active_path
        } else if self.store.exists(&parked_path)? {
            parked_path
        } else {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{active_path}{}",
                self.available_projects_hint()
            ))));
        };
        let raw = self.store.read_file(&path)?;
        let (fm, body) = Frontmatter::parse(&raw)?;
        let project = ProjectFrontmatter::try_from(fm)?;
        Ok((project, body.to_owned()))
    }

    // -----------------------------------------------------------------
    // daily_log_mentions
    // -----------------------------------------------------------------

    /// Log lines that wikilink the project (`[[<slug>]]` or
    /// `[[projects/<slug>]]`), across every daily note from `since`
    /// through the latest daily on disk. Sorted oldest-first.
    pub fn daily_log_mentions(
        &self,
        project_slug: &str,
        since: NaiveDate,
    ) -> Result<Vec<DailyLogLine>, DomainError> {
        let mut out = Vec::new();
        for entry in self.index.list_by_type(NoteType::Daily.as_str())? {
            let Some(date) = daily_note_date(&entry.path) else {
                continue;
            };
            if date < since {
                continue;
            }
            let raw = self.store.read_file(&entry.path)?;
            let doc = MarkdownDocument::parse(raw)?;
            let Ok(section) = doc.section(DAILY_LOGS_SECTION) else {
                continue;
            };
            for (time, text) in parse_log_lines(section) {
                if mentions_project(&text, project_slug) {
                    out.push(DailyLogLine { date, time, text });
                }
            }
        }
        out.sort_by(|a, b| a.date.cmp(&b.date).then(a.time.cmp(&b.time)));
        Ok(out)
    }

    // -----------------------------------------------------------------
    // project_backlinks
    // -----------------------------------------------------------------

    /// Wikilink-backlinks to the project's map, grouped by source
    /// note type. Uses the index's `links` table — no body
    /// re-parsing.
    ///
    /// **Scope limitation:** the index extracts wikilinks from the
    /// body only (see `cdno-core/src/reconcile.rs`). Frontmatter
    /// wikilinks — e.g. a portfolio's `project: "[[projects/foo]]"`
    /// field, or an evidence note's `origin:` — are *not* indexed
    /// today and therefore not returned here. Body-level references
    /// like a question's `## Related Projects - [[projects/foo]]`
    /// section work as expected. Surfacing the frontmatter links
    /// would need an extractor extension at the core layer.
    pub fn project_backlinks(&self, slug: &str) -> Result<ProjectBacklinks, DomainError> {
        let (project_path, _doc, _project) = self.resolve_any_project(slug)?;
        let backlinks = self.index.find_backlinks(&project_path)?;
        let mut out = ProjectBacklinks::default();
        for source in backlinks {
            let bucket = match self.index.find_by_path(&source)? {
                Some(entry) => match entry.note_type.as_str() {
                    "portfolio" => &mut out.portfolios,
                    "question" => &mut out.questions,
                    "evidence" => &mut out.evidence,
                    "action" => &mut out.actions,
                    _ => &mut out.other,
                },
                // Backlink pointing at an indexed source whose row
                // has since gone (race between query and removal).
                // Park in `other` rather than drop.
                None => &mut out.other,
            };
            bucket.push(source);
        }
        // Sort each bucket for deterministic output. VaultPath
        // isn't Ord; compare on the underlying Path.
        let by_path = |a: &VaultPath, b: &VaultPath| a.as_path().cmp(b.as_path());
        for bucket in [
            &mut out.portfolios,
            &mut out.questions,
            &mut out.evidence,
            &mut out.actions,
            &mut out.other,
        ] {
            bucket.sort_by(by_path);
        }
        Ok(out)
    }

    // -----------------------------------------------------------------
    // list_tracking
    // -----------------------------------------------------------------

    /// Tracking notes for `stewardship`, filtered by `activity` when
    /// supplied and by the inclusive `[from, to]` date window.
    /// Sorted most-recent-first; ties broken by activity then path.
    pub fn list_tracking(
        &self,
        stewardship: &str,
        activity: Option<&str>,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<TrackingEntry>, DomainError> {
        let entries = self.index.list_by_type(NoteType::Tracking.as_str())?;
        let mut out = Vec::new();
        for entry in entries {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, body) = Frontmatter::parse(&raw)?;
            let tf = TrackingFrontmatter::try_from(fm)?;
            if tf.stewardship != stewardship {
                continue;
            }
            if let Some(a) = activity
                && tf.activity != a
            {
                continue;
            }
            if tf.date < from || tf.date > to {
                continue;
            }
            out.push(TrackingEntry {
                path: entry.path,
                stewardship: tf.stewardship,
                activity: tf.activity,
                date: tf.date,
                duration_min: tf.duration_min,
                routine: tf.routine,
                body_excerpt: body_excerpt(body),
            });
        }
        out.sort_by(|a, b| {
            b.date
                .cmp(&a.date)
                .then_with(|| a.activity.cmp(&b.activity))
                .then_with(|| a.path.as_path().cmp(b.path.as_path()))
        });
        Ok(out)
    }

    // -----------------------------------------------------------------
    // tracking_series
    // -----------------------------------------------------------------

    /// Numeric time series for `stewardship`'s tracking notes, one
    /// series per `(activity, table column)` pair that ever carries a
    /// number — the data behind trend charts ("weight over time",
    /// "session volume per week").
    ///
    /// For each tracking note, the **first** table in the body is
    /// parsed (via `cdno-core`'s extractor — markdown structure stays
    /// out of this layer) and each column's parseable numeric cells
    /// are **summed** into one point at the note's date. Summing is
    /// the useful aggregate for both table shapes our templates
    /// produce: a single-row measurement table sums to the value
    /// itself, and a multi-row session table (one row per exercise)
    /// sums to the session total.
    ///
    /// Non-numeric cells and columns that never parse are skipped
    /// silently — tables carry prose columns (`Notes`, `Exercise`) by
    /// design. Notes without a table contribute no points. Series are
    /// sorted by name, points by date.
    pub fn tracking_series(&self, stewardship: &str) -> Result<Vec<TrackingSeries>, DomainError> {
        // BTreeMap so series come out name-sorted without a second pass.
        let mut by_name: BTreeMap<String, Vec<TrackingPoint>> = BTreeMap::new();
        for entry in self.index.list_by_type(NoteType::Tracking.as_str())? {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, body) = Frontmatter::parse(&raw)?;
            let tf = TrackingFrontmatter::try_from(fm)?;
            if tf.stewardship != stewardship {
                continue;
            }
            let Some(table) = extract_first_table(body) else {
                continue;
            };
            for (col, header) in table.headers.iter().enumerate() {
                let mut sum = 0.0;
                let mut seen_numeric = false;
                for row in &table.rows {
                    if let Some(value) = row.get(col).and_then(|cell| cell.parse::<f64>().ok()) {
                        sum += value;
                        seen_numeric = true;
                    }
                }
                if !seen_numeric {
                    continue;
                }
                by_name
                    .entry(format!(
                        "{activity} \u{b7} {header}",
                        activity = tf.activity
                    ))
                    .or_default()
                    .push(TrackingPoint {
                        date: tf.date,
                        value: sum,
                    });
            }
        }

        Ok(by_name
            .into_iter()
            .map(|(name, mut points)| {
                points.sort_by_key(|p| p.date);
                TrackingSeries { name, points }
            })
            .collect())
    }
}

// ---------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------

/// Find the Monday of the ISO-8601 week containing `date`. Use ISO
/// week (Mon-Sun) rather than locale week so behaviour is identical
/// regardless of where the binary runs.
fn monday_of_iso_week(date: NaiveDate) -> NaiveDate {
    let days_since_monday = date.weekday().num_days_from_monday() as i64;
    date - Duration::days(days_since_monday)
}

/// Convert "today minus N days" into a nanosecond timestamp suitable
/// for comparing against `NoteEntry.mtime_ns`. Anything with
/// `mtime_ns <= threshold` was last touched on or before that day.
fn mtime_threshold_ns(today: NaiveDate, days: i64) -> u64 {
    let cutoff = today - Duration::days(days);
    let datetime = cutoff
        .and_hms_opt(23, 59, 59)
        .expect("23:59:59 is always a valid time");
    let nanos = datetime.and_utc().timestamp_nanos_opt().unwrap_or(0);
    nanos.max(0) as u64
}

/// Extract the date from a daily-note path, e.g.
/// `journal/2026/daily/2026-04-06.md` → `2026-04-06`. Returns
/// `None` for paths that don't fit the daily-note shape.
fn daily_note_date(path: &VaultPath) -> Option<NaiveDate> {
    let stem = path.as_path().file_stem()?.to_str()?;
    NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()
}

/// Pull the filename stem from a vault path.
fn path_stem(path: &VaultPath) -> String {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned()
}

/// Parse a `## Logs` section into `(time, text)` pairs. Each entry
/// starts with `- **HH:MM**:`; indented continuation lines are
/// folded into the `text` separated by `; ` so the caller gets a
/// flat list. Unparseable lines are skipped silently.
fn parse_log_lines(section: &str) -> Vec<(NaiveTime, String)> {
    let mut out: Vec<(NaiveTime, String)> = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("- **") {
            // `HH:MM**: text` after the prefix.
            let Some((hhmm, after)) = rest.split_once("**: ") else {
                continue;
            };
            let Ok(time) = NaiveTime::parse_from_str(hhmm, "%H:%M") else {
                continue;
            };
            out.push((time, after.to_owned()));
        } else if !trimmed.is_empty()
            && trimmed.starts_with(' ')
            && let Some((_, prev)) = out.last_mut()
        {
            // Continuation of the previous entry. Trim leading
            // whitespace and append with a delimiter.
            prev.push_str("; ");
            prev.push_str(trimmed.trim_start());
        }
    }
    out
}

/// Recognise `was → now` project-state changes inside a `## Logs`
/// section. Returns `(project_slug, was, now)` per match. The format
/// is the one `Vault::update_project_state` writes:
///
/// ```text
/// - **HH:MM**: state on [[<slug>]]
///   was: <old_state>
///   now: <new_state>
/// ```
///
/// Multi-line state bodies are already collapsed by the writer
/// (`flatten_for_log`), so each `was:`/`now:` continuation is one
/// physical line we can match.
fn parse_state_changes(section: &str) -> Vec<(String, String, String)> {
    // Use a state machine over lines. For each `state on [[slug]]`
    // line, look for the next two indented `was:` / `now:` lines.
    let lines: Vec<&str> = section.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_end();
        // The header has both a leading `- **HH:MM**:` and the
        // `state on [[...]]` body. We don't need the time.
        let body = match line.strip_prefix("- **") {
            Some(rest) => rest.split_once("**: ").map(|(_, b)| b),
            None => None,
        };
        if let Some(body) = body
            && let Some(slug) = body
                .strip_prefix("state on [[")
                .and_then(|s| s.strip_suffix("]]"))
        {
            // Look for `  was:` then `  now:` on the next two lines.
            let was = lines
                .get(i + 1)
                .and_then(|l| l.trim_start().strip_prefix("was:"))
                .map(|s| s.trim().to_owned());
            let now = lines
                .get(i + 2)
                .and_then(|l| l.trim_start().strip_prefix("now:"))
                .map(|s| s.trim().to_owned());
            if let (Some(was), Some(now)) = (was, now) {
                out.push((slug.to_owned(), was, now));
                i += 3;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `true` if `text` contains a wikilink referencing the project.
/// Recognises both bare (`[[<slug>]]`) and folder-qualified
/// (`[[projects/<slug>]]`) shapes — the daily-log writers use both
/// depending on context.
fn mentions_project(text: &str, slug: &str) -> bool {
    let bare = format!("[[{slug}]]");
    let qualified = format!("[[projects/{slug}]]");
    text.contains(&bare) || text.contains(&qualified)
}

/// Extract the H1 text from a body. `None` when the body has no
/// `# Heading` line.
fn extract_h1(body: &str) -> Option<String> {
    body.lines().find_map(|l| {
        l.trim_start()
            .strip_prefix("# ")
            .map(|t| t.trim().to_owned())
    })
}

/// First non-blank line of the body (skipping any H1), trimmed and
/// capped to 200 chars. Bounded so a verbose body doesn't blow up
/// the MCP response payload.
fn body_excerpt(body: &str) -> String {
    let mut found_h1 = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !found_h1 && trimmed.starts_with("# ") {
            found_h1 = true;
            continue;
        }
        let mut out = trimmed.to_owned();
        if out.chars().count() > 200 {
            out = out.chars().take(200).collect::<String>() + "…";
        }
        return out;
    }
    String::new()
}
