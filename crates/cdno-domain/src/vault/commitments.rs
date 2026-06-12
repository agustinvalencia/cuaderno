//! Standalone commitment notes: create, complete.
//!
//! See `docs/design.md` §5.9. Active commitments live at
//! `commitments/<slug>.md`; completed ones move to
//! `commitments/_done/<year>/<slug>.md` with the `status` and
//! `completed` frontmatter fields stamped in the same transaction.
//!
//! Frontmatter carries `status`, `created`, and `completed` so the
//! commitments aggregation query (#32) and weekly/monthly reviews
//! can run as index lookups rather than filesystem walks.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{
    ActionFrontmatter, ActionStatus, CommitmentFrontmatter, CommitmentStatus, Context,
};
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::projects::rewrite_field_in_frontmatter;
use super::slug::slugify;
use super::stewardships::{PERIODIC_COMMITMENTS_SECTION, stewardship_slug_from_path};

const COMMITMENT_TEMPLATE: &str = include_str!("../../templates/commitment.md");

/// Fixed look-back window for surfacing overdue commitments. Anything
/// missed more than this many days ago drops out of the view rather
/// than accumulating unbounded history.
const OVERDUE_LOOKBACK_DAYS: i64 = 30;

/// One dated commitment in the aggregated view produced by
/// [`Vault::commitments`]. The `source` records where it came from so
/// callers (orient, the commitments CLI) can group or label entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitmentEntry {
    pub date: NaiveDate,
    pub title: String,
    pub source: CommitmentSource,
    /// `true` when `date` is strictly before the query's `today`.
    pub is_overdue: bool,
}

/// Origin of an aggregated commitment. The string payloads carry the
/// owning project / stewardship slug for context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitmentSource {
    /// A hard `## Milestones` deadline of the named project.
    ProjectMilestone(String),
    /// A periodic commitment of the named stewardship (Phase 3 — no
    /// stewardship notes exist yet, so this variant is currently
    /// unproduced).
    Stewardship(String),
    /// A standalone `commitments/<slug>.md` note.
    StandaloneCommitment,
    /// An action note carrying a self-imposed `due:` that isn't pinned
    /// to a milestone; the payload is the parent project slug.
    ActionNote(String),
}

impl Vault {
    /// Create a new active commitment at `commitments/<slug>.md` and
    /// log the creation to today's daily note in a single committed
    /// transaction.
    ///
    /// `at` provides both the timestamp for the daily-log entry and
    /// the date stamped in the `created:` frontmatter field. `due`
    /// is the deadline; the commitments aggregation query (#32) reads
    /// it from the index. `project` and `stewardship` are always
    /// `null` for the standalone case this function handles —
    /// originating commitments (project milestones, stewardship
    /// periodic commitments) are tracked inline at their source per
    /// design.md §5.9.
    ///
    /// Errors only on slug collisions: if `commitments/<slug>.md`
    /// already exists, returns [`StoreError::AlreadyExists`].
    /// Completed commitments at `commitments/_done/<year>/<slug>.md`
    /// don't block — slugs only need to be unique among active
    /// commitments.
    pub fn create_commitment(
        &self,
        at: NaiveDateTime,
        title: &str,
        due: NaiveDate,
        context: Context,
    ) -> Result<VaultPath, DomainError> {
        let title = title.trim();
        let slug = slugify(title);
        let path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::COMMITMENTS))?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }

        let created = at.date();
        let content = render_commitment_template(title, due, created, context);
        let entry_meta = build_index_entry_for(&path, &content, NoteType::Commitment.as_str())?;

        let log_entry = format!(
            "commitment created [[{slug}]] \u{2014} {title} (due {due})",
            due = due.format("%Y-%m-%d")
        );

        let mut tx = self.transaction();
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Mark an active commitment as completed: rewrite its
    /// `status:` and `completed:` frontmatter fields, move it to
    /// `commitments/_done/<year>/<slug>.md` (creating the year
    /// subdirectory if absent), and log the completion to today's
    /// daily note. All in a single committed transaction.
    ///
    /// The completion year comes from `at.date().year()` rather than
    /// the commitment's own `created` year, so a commitment finished
    /// in 2027 lands under `_done/2027/` regardless of when it was
    /// created.
    ///
    /// Errors:
    /// - [`StoreError::NotFound`] — slug doesn't resolve to
    ///   `commitments/<slug>.md`.
    /// - [`DomainError::CommitmentNotActive`] — file exists but its
    ///   frontmatter `status` is not `active` (defensive against drift).
    /// - [`StoreError::AlreadyExists`] — destination at
    ///   `commitments/_done/<year>/<slug>.md` is already occupied.
    pub fn complete_commitment(
        &self,
        at: NaiveDateTime,
        slug: &str,
    ) -> Result<VaultPath, DomainError> {
        let active_path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::COMMITMENTS))?;
        if !self.store.exists(&active_path)? {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{active_path}{}",
                self.available_commitments_hint()
            ))));
        }

        let raw = self.store.read_file(&active_path)?;
        // Defensive frontmatter check: the file is at
        // `commitments/<slug>.md`, but a manual edit could have set
        // status to completed. Trust the frontmatter, refuse if it's
        // not active.
        let (fm, _body) = Frontmatter::parse(&raw)?;
        let commitment = CommitmentFrontmatter::try_from(fm)?;
        if commitment.status != CommitmentStatus::Active {
            return Err(DomainError::CommitmentNotActive(slug.to_owned()));
        }

        let completion_date = at.date();
        let year = completion_date.year();
        let done_dir = cdno_core::paths::commitments_done_dir(year);
        let done_path = VaultPath::new(format!("{done_dir}/{slug}.md"))?;
        if self.store.exists(&done_path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                done_path.to_string(),
            )));
        }

        // Rewrite both lifecycle fields in the frontmatter. The
        // helper preserves every other line — comments, key order,
        // user notes — only `status` and `completed` are touched.
        let after_status =
            rewrite_field_in_frontmatter(&raw, "status", CommitmentStatus::Completed.as_str())?;
        let new_content = rewrite_field_in_frontmatter(
            &after_status,
            "completed",
            &completion_date.format("%Y-%m-%d").to_string(),
        )?;
        let entry_meta =
            build_index_entry_for(&done_path, &new_content, NoteType::Commitment.as_str())?;

        let title_for_log = body_title_or_slug(&new_content, slug);
        let log_entry = format!("commitment completed [[{slug}]] \u{2014} {title_for_log}");

        let mut tx = self.transaction();
        tx.write_file(done_path.clone(), new_content);
        tx.delete_file(active_path.clone());
        tx.upsert_note(entry_meta);
        tx.remove_note(active_path);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(done_path)
    }

    /// " — available commitments: …" suffix for a commitment slug
    /// not-found, listing the *open* commitments (those still at
    /// `commitments/<slug>.md`). Fulfilled ones live under
    /// `commitments/_done/<year>/` and can't be completed again, so any
    /// path with a `_done` component is skipped. See
    /// [`slug_hint::available_slugs_hint`](super::slug_hint::available_slugs_hint).
    fn available_commitments_hint(&self) -> String {
        super::slug_hint::available_slugs_hint(
            self.index.as_ref(),
            NoteType::Commitment.as_str(),
            "commitments",
            |path| {
                if path
                    .as_path()
                    .components()
                    .any(|c| c.as_os_str() == "_done")
                {
                    return None;
                }
                let slug = path.as_path().file_stem()?.to_str()?.to_owned();
                Some((slug.clone(), slug))
            },
        )
    }

    /// Aggregate every dated commitment across the vault into one
    /// date-sorted view, from a fixed 30-day overdue look-back through
    /// `today + lookahead_days`.
    ///
    /// Four sources (design §5.9 / §5.11):
    /// 1. Hard project milestones, read from the `milestones` index
    ///    table (#109) — completed and soft milestones are skipped.
    /// 2. Stewardship periodic commitments — absent until Phase 3, so
    ///    currently contributes nothing.
    /// 3. Standalone active commitment notes.
    /// 4. Active action notes with a self-imposed `due:` and **no**
    ///    `milestone:`. A milestone-pinned action is *not* duplicated
    ///    here — its milestone (source 1) owns the date.
    ///
    /// Each entry is flagged `is_overdue` when its date is before
    /// `today`. Sources 3 and 4 read each note's file to parse the
    /// typed frontmatter (the established query pattern); a malformed
    /// note fails the whole query rather than being silently dropped.
    pub fn commitments(
        &self,
        today: NaiveDate,
        lookahead_days: i64,
    ) -> Result<Vec<CommitmentEntry>, DomainError> {
        let from = today - Duration::days(OVERDUE_LOOKBACK_DAYS);
        let to = today + Duration::days(lookahead_days);

        let mut entries = Vec::new();

        // Source 1: hard project milestones via the index table. The
        // query already bounds by date and excludes undated markers.
        let from_s = from.format("%Y-%m-%d").to_string();
        let to_s = to.format("%Y-%m-%d").to_string();
        for (path, milestone) in self.index.milestones_between(&from_s, &to_s)? {
            if !milestone.is_hard || milestone.completed {
                continue;
            }
            let Some(date) = milestone.date.as_deref().and_then(parse_ymd) else {
                continue;
            };
            entries.push(CommitmentEntry {
                date,
                title: milestone.name,
                source: CommitmentSource::ProjectMilestone(slug_of(&path)),
                is_overdue: date < today,
            });
        }

        // Source 2: stewardship periodic commitments. Parse each
        // stewardship's `## Periodic Commitments` section; a line
        // whose `next:` date falls inside the [from, to] window is
        // surfaced as one commitment entry. Malformed lines are
        // skipped (lint is the place to surface them) so a single
        // typo doesn't break the whole aggregation.
        for entry in self.index.list_by_type(NoteType::Stewardship.as_str())? {
            let raw = self.store.read_file(&entry.path)?;
            let slug = stewardship_slug_from_path(&entry.path);
            let Ok(doc) = MarkdownDocument::parse(raw) else {
                continue;
            };
            let Ok(section) = doc.section(PERIODIC_COMMITMENTS_SECTION) else {
                continue;
            };
            for line in section.lines() {
                let Some((title, next)) = parse_periodic_line(line) else {
                    continue;
                };
                if next < from || next > to {
                    continue;
                }
                entries.push(CommitmentEntry {
                    date: next,
                    title,
                    source: CommitmentSource::Stewardship(slug.clone()),
                    is_overdue: next < today,
                });
            }
        }

        // Source 3: standalone active commitment notes.
        for entry in self.index.list_by_type(NoteType::Commitment.as_str())? {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let commitment = CommitmentFrontmatter::try_from(fm)?;
            if commitment.status != CommitmentStatus::Active
                || commitment.due < from
                || commitment.due > to
            {
                continue;
            }
            let slug = slug_of(&entry.path);
            entries.push(CommitmentEntry {
                date: commitment.due,
                title: body_title_or_slug(&raw, &slug).to_owned(),
                source: CommitmentSource::StandaloneCommitment,
                is_overdue: commitment.due < today,
            });
        }

        // Source 4: active action notes with a self-imposed due and no
        // milestone pin (milestone-pinned actions are covered by their
        // milestone in source 1).
        for entry in self.index.list_by_type(NoteType::Action.as_str())? {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let action = ActionFrontmatter::try_from(fm)?;
            let Some(due) = action.due else { continue };
            if action.status != ActionStatus::Active
                || action.milestone.is_some()
                || due < from
                || due > to
            {
                continue;
            }
            let slug = slug_of(&entry.path);
            entries.push(CommitmentEntry {
                date: due,
                title: body_title_or_slug(&raw, &slug).to_owned(),
                source: CommitmentSource::ActionNote(action.project),
                is_overdue: due < today,
            });
        }

        entries.sort_by_key(|entry| entry.date);
        Ok(entries)
    }
}

/// Parse an ISO `YYYY-MM-DD` date, returning `None` for any other
/// shape. Index dates are validated on the way in, so this is
/// belt-and-braces for the source-1 path.
fn parse_ymd(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// The slug of a note: its file stem. Paths in the index always have a
/// `.md` stem, so the fallback is unreachable in practice.
fn slug_of(path: &VaultPath) -> String {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned()
}

/// Render the built-in commitment template with all fields stamped.
/// The template is the wire format — see `templates/commitment.md`.
fn render_commitment_template(
    title: &str,
    due: NaiveDate,
    created: NaiveDate,
    context: Context,
) -> String {
    COMMITMENT_TEMPLATE
        .replace("{{title}}", title)
        .replace("{{status}}", CommitmentStatus::Active.as_str())
        .replace("{{due}}", &due.format("%Y-%m-%d").to_string())
        .replace("{{created}}", &created.format("%Y-%m-%d").to_string())
        // `null` for the optional fields — both the completion date
        // and the project/stewardship origin links are absent for a
        // freshly-created standalone commitment.
        .replace("{{completed}}", "null")
        .replace("{{context}}", context.as_str())
        .replace("{{project}}", "null")
        .replace("{{stewardship}}", "null")
}

/// Pull the heading text from the rendered body for the daily-log
/// entry. Looks for the first `# ` heading; falls back to the slug
/// if absent (shouldn't happen for templates we wrote, but the log
/// shouldn't crash on a hand-edited oddity).
fn body_title_or_slug<'a>(content: &'a str, slug: &'a str) -> &'a str {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            return rest.trim();
        }
    }
    slug
}

/// Parse one line of a `## Periodic Commitments` section into
/// `(title, next_date)`. Returns `None` for anything that doesn't fit
/// the canonical shape from design §5.6:
///
/// ```text
/// - Title \u{2014} <recurrence> \u{2014} next: YYYY-MM-DD
/// ```
///
/// We discard the recurrence on the parse side — the aggregator only
/// cares about *when* the next occurrence is due. Trailing
/// `(overdue)` is tolerated and stripped before the date parse so
/// hand-annotated lines still round-trip.
fn parse_periodic_line(line: &str) -> Option<(String, NaiveDate)> {
    let rest = line.trim_start().strip_prefix("- ")?;
    let parts: Vec<&str> = rest.splitn(3, '\u{2014}').collect();
    if parts.len() != 3 {
        return None;
    }
    let title = parts[0].trim().to_owned();
    if title.is_empty() {
        return None;
    }
    let next_part = parts[2].trim();
    let after_marker = next_part.strip_prefix("next:")?.trim();
    // Strip a trailing `(overdue)` annotation if present so the
    // remainder is a clean date string.
    let date_str = after_marker
        .split_whitespace()
        .next()
        .unwrap_or(after_marker);
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    Some((title, date))
}
