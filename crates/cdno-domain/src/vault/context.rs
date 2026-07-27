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

use std::collections::{BTreeMap, BTreeSet};

use cdno_core::config::{Aggregate, TrackingSpec};
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
use super::projects::actions::{LOG_ACTION_DONE_PREFIX, LOG_STARTED_PREFIX};

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
    pub portfolios: Vec<BacklinkRef>,
    pub questions: Vec<BacklinkRef>,
    pub evidence: Vec<BacklinkRef>,
    pub actions: Vec<BacklinkRef>,
    /// Anything else (commitments, daily notes, hand-edited
    /// references). Lets the caller still render every backlink even
    /// when the source type isn't one of the call-out groups.
    pub other: Vec<BacklinkRef>,
}

/// One note that links to the subject, with what a reader needs to
/// recognise it.
///
/// A path alone is a poor label — `portfolios/how-should-the-pipeline-be-staged/2026-07-13-index-shape.md`
/// says less at a glance than the note's own title — and gives no basis
/// for ordering, so a long list arrives in whatever order the index
/// happened to yield.
///
/// (`QuestionBacklinks` still carries bare paths. Its only consumer is the
/// Strategic view, which is being reworked separately; converting it here
/// would churn a file that is about to be rewritten.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklinkRef {
    pub path: VaultPath,
    /// The source note's title, when the index has one. Absent for a note
    /// with no H1 or title frontmatter — the caller falls back to the path.
    pub title: Option<String>,
    /// Filesystem mtime, the key the buckets are sorted on (newest first).
    /// Carried so a caller can show recency without a second lookup.
    pub modified_ns: u64,
}

/// Backlinks to a question, grouped by source note type (#354) — the
/// question-side mirror of [`ProjectBacklinks`]. Both body wikilinks and
/// frontmatter wikilinks are indexed (#395), so a project that answers this
/// question via `core_question:` appears in `projects` here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuestionBacklinks {
    pub projects: Vec<VaultPath>,
    pub portfolios: Vec<VaultPath>,
    pub evidence: Vec<VaultPath>,
    /// Anything else (actions, commitments, daily notes, hand-edited
    /// references) — so a consumer can still render every backlink even
    /// when the source type isn't one of the call-out groups.
    pub other: Vec<VaultPath>,
}

/// One numeric time series lifted from a stewardship's tracking
/// notes, ready for a trend chart. See [`Vault::tracking_series`].
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TrackingSeries {
    /// `"{activity} · {column header}"` — e.g. `"gym · Weight (kg)"`.
    pub name: String,
    /// One point per tracking note that had a numeric value in the
    /// column, sorted by date.
    pub points: Vec<TrackingPoint>,
}

/// One dated value in a [`TrackingSeries`].
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
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
    // stuck_project_days
    // -----------------------------------------------------------------

    /// The same active-project staleness filter as
    /// [`Vault::stuck_projects`], but returning each stuck project's
    /// slug paired with the whole number of days since its map was last
    /// modified. The weekly-review scan (#55) renders this as the grey
    /// "state untouched for N days" hint, which needs the count, not
    /// just the set — so it can't lean on `stuck_projects`
    /// (`ProjectSummary`, no age) and keeps its own thin loop rather
    /// than widening that summary type for one caller.
    pub fn stuck_project_days(
        &self,
        today: NaiveDate,
        unchanged_for_days: i64,
    ) -> Result<Vec<(String, i64)>, DomainError> {
        let threshold_ns = mtime_threshold_ns(today, unchanged_for_days);
        let entries = self.index.list_by_type(NoteType::Project.as_str())?;
        let mut out = Vec::new();
        for entry in entries {
            // Same active-only gate as stuck_projects: the "stuck"
            // heuristic is meaningless for parked / completed work.
            let raw = self.store.read_file(&entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let pf = ProjectFrontmatter::try_from(fm)?;
            if pf.status != ProjectStatus::Active {
                continue;
            }
            if entry.mtime_ns > threshold_ns {
                continue;
            }
            out.push((
                path_stem(&entry.path),
                days_since_mtime(today, entry.mtime_ns),
            ));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
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
    /// The index extracts wikilinks from both the body and the
    /// frontmatter (#395), so a portfolio's `project: "[[projects/foo]]"`
    /// field and an evidence note's `origin:` are returned here alongside
    /// body-level references (e.g. a question's `## Related Projects -
    /// [[projects/foo]]` section).
    pub fn project_backlinks(&self, slug: &str) -> Result<ProjectBacklinks, DomainError> {
        let (project_path, _doc, _project) = self.resolve_any_project(slug)?;
        let backlinks = self.index.find_backlinks(&project_path)?;
        let mut out = ProjectBacklinks::default();
        for source in backlinks {
            // One lookup serves both the bucketing and the label: the entry
            // already carries the title and mtime, so enriching the row
            // costs no extra I/O.
            let entry = self.index.find_by_path(&source)?;
            let bucket = match &entry {
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
            bucket.push(BacklinkRef {
                path: source,
                title: entry.as_ref().and_then(|e| e.title.clone()),
                modified_ns: entry.as_ref().map_or(0, |e| e.mtime_ns),
            });
        }
        // Newest first: a project accrues backlinks for as long as it runs,
        // and the recent ones are the context a reader wants. Path breaks
        // ties so the order stays deterministic (VaultPath isn't Ord, so
        // compare the underlying Path).
        for bucket in [
            &mut out.portfolios,
            &mut out.questions,
            &mut out.evidence,
            &mut out.actions,
            &mut out.other,
        ] {
            bucket.sort_by(|a, b| {
                b.modified_ns
                    .cmp(&a.modified_ns)
                    .then_with(|| a.path.as_path().cmp(b.path.as_path()))
            });
        }
        Ok(out)
    }

    // -----------------------------------------------------------------
    // question_backlinks
    // -----------------------------------------------------------------

    /// Backlinks to a question, grouped by source note type (#354) — the
    /// question-side mirror of [`project_backlinks`](Self::project_backlinks),
    /// reusing the index's `find_backlinks`. The strategic questions grid
    /// renders these as project / evidence chips alongside the portfolio
    /// chips. Both body and frontmatter wikilinks are indexed (#395), so a
    /// project that answers this question via `core_question:` lands in the
    /// `projects` bucket, as does a body reference (a portfolio's or evidence
    /// note's `[[questions/…]]` wikilink).
    pub fn question_backlinks(&self, slug: &str) -> Result<QuestionBacklinks, DomainError> {
        let (question_path, _qf) = self.resolve_question_by_slug(slug)?;
        let backlinks = self.index.find_backlinks(&question_path)?;
        let mut out = QuestionBacklinks::default();
        for source in backlinks {
            let bucket = match self.index.find_by_path(&source)? {
                Some(entry) => match entry.note_type.as_str() {
                    "project" => &mut out.projects,
                    "portfolio" => &mut out.portfolios,
                    "evidence" => &mut out.evidence,
                    _ => &mut out.other,
                },
                // Backlink source whose index row has since gone (race
                // between query and removal). Park in `other`, don't drop.
                None => &mut out.other,
            };
            bucket.push(source);
        }
        // Deterministic output. VaultPath isn't Ord; compare on Path.
        let by_path = |a: &VaultPath, b: &VaultPath| a.as_path().cmp(b.as_path());
        for bucket in [
            &mut out.projects,
            &mut out.portfolios,
            &mut out.evidence,
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
    /// number — the data behind trend charts ("weight over time").
    ///
    /// For each tracking note, the **first** table in the body is
    /// parsed (via `cdno-core`'s extractor — markdown structure stays
    /// out of this layer) and each column's parseable numeric cells
    /// are **summed** into one point at the note's date. Per-column
    /// sums are the canonical *raw* aggregate this layer can compute
    /// without knowing column semantics: a single-row measurement
    /// table sums to the value itself, and multi-row columns sum to
    /// their per-note total (meaningful for counts like Sets/Reps;
    /// noise for e.g. Weight-per-exercise — picking which series to
    /// chart is the consumer's job).
    ///
    /// Non-numeric and non-finite cells (`NaN`/`inf` parse as f64 but
    /// would poison sums and serialise as JSON `null`) and columns
    /// that never yield a value are skipped silently — tables carry
    /// prose columns (`Notes`, `Exercise`) by design. Notes without a
    /// table contribute no points. Series are sorted by name, points
    /// by date.
    pub fn tracking_series(&self, stewardship: &str) -> Result<Vec<TrackingSeries>, DomainError> {
        self.tracking_series_excluding(stewardship, &BTreeSet::new())
    }

    /// As [`tracking_series`](Self::tracking_series), skipping any activity in
    /// `declared` — the body-table half of the precedence rule (`#485`).
    fn tracking_series_excluding(
        &self,
        stewardship: &str,
        declared: &BTreeSet<&str>,
    ) -> Result<Vec<TrackingSeries>, DomainError> {
        // BTreeMap so series come out name-sorted without a second pass.
        let mut by_name: BTreeMap<String, Vec<TrackingPoint>> = BTreeMap::new();
        for entry in self.index.list_by_type(NoteType::Tracking.as_str())? {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, body) = Frontmatter::parse(&raw)?;
            let tf = TrackingFrontmatter::try_from(fm)?;
            if tf.stewardship != stewardship {
                continue;
            }
            if declared.contains(tf.activity.as_str()) {
                continue;
            }
            let Some(table) = extract_first_table(body) else {
                continue;
            };
            for (col, header) in table.headers.iter().enumerate() {
                let mut sum = 0.0;
                let mut seen_numeric = false;
                for row in &table.rows {
                    if let Some(value) = row
                        .get(col)
                        .and_then(|cell| cell.parse::<f64>().ok())
                        .filter(|v| v.is_finite())
                    {
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

    /// Derive tracking series from **frontmatter**, reducing each metric by
    /// its own declared [`Aggregate`] (`#483`).
    ///
    /// The successor to [`tracking_series`](Self::tracking_series), which
    /// re-reads every file from disk, parses the first body table, and sums
    /// every numeric column. Summing is right for a total and wrong for
    /// everything else, and a column cannot be split by an entity — so this
    /// reads the parsed frontmatter the index already holds (no
    /// `store.read_file`), applies the metric's own reduction, and can fan one
    /// entry out into a series per group value.
    ///
    /// `specs` is keyed by activity. An activity with no spec contributes
    /// nothing here, and keeps working through the body-table engine
    /// unchanged — declaring is opt-in and no migration is forced. An activity
    /// whose spec declares no metrics is a complete, valid use (a cadence-only
    /// occurrence) and likewise yields no series.
    ///
    /// Series names are formatted only at the end, as
    /// `"<activity> · [<group> · ]<metric>"`. The accumulator is keyed on a
    /// typed tuple rather than that string, because a group value may
    /// legitimately contain the separator.
    ///
    /// A date a group was not recorded on produces **no point** — a gap, never
    /// a zero, since zero-filling would draw a false line to the axis. A group
    /// value appearing for the first time starts its own series there, with no
    /// config change.
    pub fn tracking_series_from_frontmatter(
        &self,
        stewardship: &str,
        specs: &BTreeMap<String, TrackingSpec>,
    ) -> Result<Vec<TrackingSeries>, DomainError> {
        // Values are pushed in document order; `last` depends on it.
        let mut acc: BTreeMap<SeriesKey, BTreeMap<NaiveDate, Vec<f64>>> = BTreeMap::new();

        for entry in self.index.list_by_type(NoteType::Tracking.as_str())? {
            let fm = &entry.frontmatter;
            if str_field(fm, "stewardship") != Some(stewardship) {
                continue;
            }
            let (Some(activity), Some(date)) = (str_field(fm, "activity"), date_field(fm)) else {
                continue;
            };
            let Some(spec) = specs.get(activity) else {
                continue;
            };

            for record in records_of(fm, spec) {
                for (metric, mspec) in &spec.metrics {
                    // A grouped metric needs the group field to attribute the
                    // record. A record missing it is dropped for that metric:
                    // it belongs to no series, and inventing an "uncategorised"
                    // bucket would put a value the user did not categorise into
                    // a series they did not declare. Note this is a different
                    // case from the gap rule below — that one is about a date
                    // on which a group was not recorded; this one discards a
                    // value that WAS recorded, which is only right because
                    // there is nowhere honest to put it.
                    let group = match mspec.group_field(spec) {
                        Some(field) => match group_key(record, field) {
                            Some(g) => Some(g),
                            None => continue,
                        },
                        None => None,
                    };
                    // Non-finite values are dropped: they poison a sum and
                    // would serialise as JSON `null` downstream.
                    let Some(value) = numeric_field(record, metric).filter(|v| v.is_finite())
                    else {
                        continue;
                    };
                    acc.entry(SeriesKey {
                        activity: activity.to_owned(),
                        group,
                        metric: metric.clone(),
                    })
                    .or_default()
                    .entry(date)
                    .or_default()
                    .push(value);
                }
            }
        }

        // One reduction per (series, date) cell. With today's one-entry-per-day
        // guard the cell holds a single entry's records, so this is a
        // within-entry collapse; once same-day entries merge (#488) the cell
        // holds both and the same rule reduces across them. Merge widens what
        // the cell contains rather than adding a second level.
        let mut series: Vec<TrackingSeries> = acc
            .into_iter()
            .map(|(key, by_date)| {
                let aggregate = specs
                    .get(&key.activity)
                    .and_then(|s| s.metrics.get(&key.metric))
                    .map(|m| m.aggregate)
                    .unwrap_or_default();
                let points = by_date
                    .into_iter()
                    .map(|(date, values)| TrackingPoint {
                        date,
                        value: reduce(&values, aggregate),
                    })
                    .collect();
                TrackingSeries {
                    name: key.display_name(),
                    points,
                }
            })
            .collect();
        series.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(series)
    }

    /// Every series for `stewardship`, each activity drawn from whichever
    /// source it declares (`#485`).
    ///
    /// **A declared activity suppresses its own body-table series entirely.**
    /// Declaration is the opt-in and it is unambiguous: if the activity has a
    /// [`TrackingSpec`], its series come from frontmatter and nowhere else.
    /// Undeclared activities are untouched — the table engine serves them
    /// exactly as before, so no migration is forced.
    ///
    /// Without the rule, an activity that is declared while its notes still
    /// carry a legacy body table would emit **two** series for the same
    /// metric, under names that can collide, and the two would *disagree* —
    /// the whole point of declaring is that the table's blanket sum was wrong.
    /// That is a correctness bug rather than a migration question, which is
    /// why the rule does not wait on whether body tables should eventually go
    /// away.
    ///
    /// The specs argument is temporary: `#487` reads them from
    /// `[tracking.<activity>]` and this takes no parameter but the slug.
    pub fn tracking_series_with_specs(
        &self,
        stewardship: &str,
        specs: &BTreeMap<String, TrackingSpec>,
    ) -> Result<Vec<TrackingSeries>, DomainError> {
        let declared: BTreeSet<&str> = specs.keys().map(String::as_str).collect();
        let mut series = self.tracking_series_from_frontmatter(stewardship, specs)?;
        series.extend(self.tracking_series_excluding(stewardship, &declared)?);
        series.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(series)
    }

    /// What you are in the middle of, according to today's log.
    ///
    /// Starting an action writes `started [[slug]] - text` into the daily
    /// note and completing it writes `action done on [[slug]] - text`, so
    /// "what am I on" is already recorded. This reads it back rather than
    /// keeping a parallel piece of state that could disagree with the
    /// vault — which also means it sees a start made from the CLI or by an
    /// agent over MCP, not only one clicked in the app.
    ///
    /// The most recent start with no matching completion wins. Several
    /// starts in a day are normal — you pick something up, put it down,
    /// pick up something else — and the last one standing is what you are
    /// on.
    pub fn current_focus(&self, date: NaiveDate) -> Result<Option<CurrentFocus>, DomainError> {
        let view = self.read_daily_note(date)?;
        if !view.exists {
            return Ok(None);
        }
        let doc = MarkdownDocument::parse(view.markdown)?;
        let Ok(section) = doc.section(DAILY_LOGS_SECTION) else {
            return Ok(None);
        };

        // Walk forward keeping the open starts in order; a completion
        // clears its matching start wherever it sits, since a day can
        // interleave several.
        let mut open: Vec<CurrentFocus> = Vec::new();
        for (time, text) in parse_log_lines(section) {
            if let Some((project, action)) = parse_focus_marker(&text, LOG_STARTED_PREFIX) {
                open.push(CurrentFocus {
                    project,
                    action,
                    started: time,
                });
            } else if let Some((project, action)) =
                parse_focus_marker(&text, LOG_ACTION_DONE_PREFIX)
            {
                open.retain(|f| !(f.project == project && f.action == action));
            }
        }
        Ok(open.pop())
    }
}

/// An action started and not yet finished, as recorded in a daily log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentFocus {
    /// The project slug the action belongs to.
    pub project: String,
    /// The action text as logged, energy suffix and all.
    pub action: String,
    /// When it was started, from the log line's own stamp.
    pub started: NaiveTime,
}

/// Split `<prefix>[[project]] - action` into its project and action.
///
/// `None` for a line without the marker, or with it but not in the
/// wikilink-and-em-dash shape the writers produce: a hand-typed log line
/// that happens to begin "started something" must not register as a focus.
fn parse_focus_marker(text: &str, prefix: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix(prefix)?;
    let rest = rest.strip_prefix("[[")?;
    let (project, rest) = rest.split_once("]]")?;
    let action = rest.trim_start().strip_prefix('\u{2014}')?.trim();
    if project.is_empty() || action.is_empty() {
        return None;
    }
    Some((project.to_owned(), action.to_owned()))
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
///
/// `today` is a LOCAL calendar date (the boundary stamps it from
/// `Local::now().date_naive()`), so the cutoff is the last instant of
/// that local day taken in the machine's local zone — not UTC. Mixing a
/// local date with a UTC instant shifts the boundary by the zone offset,
/// which for a positive-offset zone just after midnight moves a file
/// modified "today" onto the wrong side of the cutoff. The system's
/// contract is machine-local dates throughout (see `clock.rs`), so the
/// instant conversion honours the same zone. During a DST "spring
/// forward" 23:59:59 always exists, so `.single()`/`.earliest()` is safe;
/// the `unwrap_or(0)` is a defensive floor, never expected.
fn mtime_threshold_ns(today: NaiveDate, days: i64) -> u64 {
    mtime_threshold_ns_in(today, days, &chrono::Local)
}

/// Timezone-injected form of [`mtime_threshold_ns`] — the boundary
/// zone is a parameter instead of hard-coded `chrono::Local`, so a
/// deterministic test can pin it to a `FixedOffset` regardless of the
/// runner's own zone or the wall-clock time (#380). Production callers
/// go through `mtime_threshold_ns`, which passes `&chrono::Local`.
#[doc(hidden)]
pub fn mtime_threshold_ns_in<Tz: chrono::TimeZone>(today: NaiveDate, days: i64, tz: &Tz) -> u64 {
    let cutoff = today - Duration::days(days);
    let datetime = cutoff
        .and_hms_opt(23, 59, 59)
        .expect("23:59:59 is always a valid time");
    let nanos = datetime
        .and_local_timezone(tz.clone())
        .earliest()
        .and_then(|dt| dt.timestamp_nanos_opt())
        .unwrap_or(0);
    nanos.max(0) as u64
}

/// Whole days from a note's `mtime_ns` (nanoseconds since the Unix
/// epoch, UTC) back from `today`. Used by `stuck_project_days` to
/// report how long a project map has sat untouched. A `mtime_ns` that
/// can't be represented as a timestamp (only possible on a corrupt
/// index row) degrades to `today`, i.e. zero days, rather than
/// panicking a read.
///
/// The mtime is a UTC instant but `today` is a LOCAL date, so the mtime
/// is converted to its LOCAL calendar date before the day subtraction —
/// otherwise a file written "today" in local time but "yesterday" in UTC
/// (any positive-offset zone in the hours after local midnight) would
/// report one stale day too many.
fn days_since_mtime(today: NaiveDate, mtime_ns: u64) -> i64 {
    days_since_mtime_in(today, mtime_ns, &chrono::Local)
}

/// Timezone-injected form of [`days_since_mtime`] — the zone the mtime
/// instant is resolved into is a parameter instead of hard-coded
/// `chrono::Local`, so a deterministic test can inject a `FixedOffset`
/// and exercise the local-vs-UTC boundary (#380, the #379 fix) without
/// depending on the runner's zone or the current time. Production
/// callers go through `days_since_mtime` with `&chrono::Local`.
#[doc(hidden)]
pub fn days_since_mtime_in<Tz: chrono::TimeZone>(today: NaiveDate, mtime_ns: u64, tz: &Tz) -> i64 {
    let secs = (mtime_ns / 1_000_000_000) as i64;
    let modified = chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.with_timezone(tz).date_naive())
        .unwrap_or(today);
    (today - modified).num_days()
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

/// Identifies one derived series (`#483`).
///
/// A typed tuple, never the formatted display name: a group value may
/// legitimately contain the `·` used as the display separator, so keying on
/// the rendered string would collide series that are genuinely distinct.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SeriesKey {
    activity: String,
    group: Option<String>,
    metric: String,
}

impl SeriesKey {
    /// `"<activity> · [<group> · ]<metric>"` — formatted once, at the end.
    fn display_name(&self) -> String {
        match &self.group {
            Some(group) => format!(
                "{activity} \u{b7} {group} \u{b7} {metric}",
                activity = self.activity,
                metric = self.metric
            ),
            None => format!(
                "{activity} \u{b7} {metric}",
                activity = self.activity,
                metric = self.metric
            ),
        }
    }
}

/// A string frontmatter field, or `None` when absent or not a string.
fn str_field<'a>(fm: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    fm.get(key)?.as_str()
}

/// The entry's `date`, parsed from its `YYYY-MM-DD` frontmatter string.
fn date_field(fm: &serde_json::Value) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(str_field(fm, "date")?, "%Y-%m-%d").ok()
}

/// The records one entry contributes, in the order they reduce.
///
/// A record activity yields its declared array; a scalar activity yields the
/// frontmatter itself as a single pseudo-record, so both shapes go through the
/// same loop.
///
/// Order is **document order** — the order the records appear in the file —
/// because `last` is the one aggregate that depends on it and no intra-day
/// time is otherwise persisted (`date` is a `NaiveDate`; the write discards
/// the time). Index iteration order must never be relied on for this:
/// `list_by_type` orders by path.
///
/// An optional per-record `at` is the escape hatch, and it is
/// **all-or-nothing**: the records reorder only when *every* one of them
/// carries an `at` that parses. A partial or unparseable set falls back to
/// document order untouched.
///
/// That strictness is the point. Ordering on a mixture would have to invent a
/// position for the unstamped records, and any choice silently changes which
/// reading `last` reports — one record scaffolded with `at: null` would
/// reorder the entry around it. Falling back keeps the failure mode "the hatch
/// did nothing" rather than "the hatch quietly picked a different number".
///
/// The key is `at`, not `time`, and the difference matters: `time` is a
/// plausible *metric* name — a swim split, a lap time, a rest interval, and
/// the repo's own swim template has a `Time` column meaning duration — and a
/// duration like `1:35` parses perfectly well as a clock time. Reserving
/// `time` would silently order a record set by one of its own measurements.
/// `at` matches the vocabulary the write surfaces already use (`cdno log
/// --at`, `cdno track --at`) for when something happened.
fn records_of<'a>(
    fm: &'a serde_json::Value,
    spec: &cdno_core::config::TrackingSpec,
) -> Vec<&'a serde_json::Value> {
    let Some(key) = spec.records.as_deref() else {
        return vec![fm];
    };
    let Some(array) = fm.get(key).and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut records: Vec<&serde_json::Value> = array.iter().collect();

    // Parse rather than compare the raw strings: `"18:00"` sorts before
    // `"9:00"` lexicographically, so a level read morning-then-evening would
    // report the morning reading as the day's last — exactly the
    // never-was-true number this whole change exists to eliminate.
    let times: Option<Vec<NaiveTime>> = records
        .iter()
        .map(|r| str_field(r, RECORD_TIME_KEY).and_then(parse_record_time))
        .collect();
    if let Some(times) = times {
        let mut keyed: Vec<(NaiveTime, &serde_json::Value)> =
            times.into_iter().zip(records.iter().copied()).collect();
        // Stable, so records sharing a time keep their document order.
        keyed.sort_by_key(|(time, _)| *time);
        records = keyed.into_iter().map(|(_, record)| record).collect();
    }
    records
}

/// The per-record key that orders a record set. See [`records_of`] for why it
/// is `at` rather than `time`.
const RECORD_TIME_KEY: &str = "at";

/// Parse a record's `at` field. Accepts 24-hour with or without seconds and
/// 12-hour with a meridiem, since the field is hand-authored and nothing in
/// the vault writes it.
fn parse_record_time(raw: &str) -> Option<NaiveTime> {
    let raw = raw.trim();
    ["%H:%M", "%H:%M:%S", "%I:%M %p", "%I:%M:%S %p"]
        .iter()
        .find_map(|format| NaiveTime::parse_from_str(raw, format).ok())
}

/// The group a record belongs to, as a display string. A number or bool is
/// rendered rather than refused — a category may legitimately be `2026` or a
/// flag — but a nested value is not a group.
fn group_key(record: &serde_json::Value, field: &str) -> Option<String> {
    match record.get(field)? {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A record's numeric value for `metric`. Accepts an integer as well as a
/// float: a round reading is written without a decimal point.
fn numeric_field(record: &serde_json::Value, metric: &str) -> Option<f64> {
    record.get(metric)?.as_f64()
}

/// Collapse one cell's values to a single point.
///
/// `max`/`min` fold with [`f64::max`]/[`f64::min`], never
/// `partial_cmp().unwrap()`, which *panics* on a single NaN — turning one bad
/// frontmatter value into a failed chart render and a failed MCP call. Values
/// are filtered for finiteness before they get here, so this is belt and
/// braces, but the panicking spelling is one refactor away from being
/// reachable.
///
/// Note `f64::max` rather than a `total_cmp`-keyed `max_by`: the two disagree
/// on exactly the input this is about. `f64::max` returns the non-NaN operand,
/// while `total_cmp` orders NaN as the greatest value and would propagate it
/// into the series (and out as JSON `null`).
fn reduce(values: &[f64], aggregate: Aggregate) -> f64 {
    match aggregate {
        Aggregate::Sum => values.iter().sum(),
        Aggregate::Mean => values.iter().sum::<f64>() / values.len() as f64,
        Aggregate::Last => values.last().copied().unwrap_or_default(),
        Aggregate::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Aggregate::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
    }
}
