//! Tracking note scaffolding (design §5.7).
//!
//! A tracking note records one occurrence of an activity under a
//! stewardship. The file lands at
//! `stewardships/<slug>/tracking/<YYYY-MM-DD>-<activity>.md` — only
//! **expanded** stewardships have a `tracking/` subdir, so a flat
//! stewardship is a hard error here (callers should know which
//! variant they're working with, or use `list_stewardships()` to
//! check).
//!
//! Activity-specific templates ship for `gym`, `body`, and `swim`
//! (matching the design's worked examples); anything else falls back
//! to the generic template. The user fleshes out the table or notes
//! after the file is created — this op writes the scaffold and gets
//! out of the way.

use chrono::{Datelike, NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::path::VaultPath;
use cdno_core::template::VariableContext;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;
use super::stewardships::StewardshipVariant;

impl Vault {
    /// File a tracking note under an expanded stewardship.
    ///
    /// The path is
    /// `stewardships/<stewardship>/tracking/<YYYY-MM-DD>-<activity-slug>.md`.
    /// Built-in templates for `gym`, `body`, and `swim` carry the
    /// design's structured shape (rep table, metrics table, swim
    /// set table); any other activity slug uses the generic template
    /// with an empty Notes section the user fills in.
    ///
    /// `content` becomes the body of the `## Notes` section. Pass
    /// `""` to leave it blank — the file is intended to be edited
    /// after creation.
    ///
    /// `routine` is an optional bare wikilink target (e.g.
    /// `"upper-body-a"`) that the domain wraps into
    /// `[[stewardships/<stewardship>/routines/<routine>]]` and
    /// substitutes for the template's `routine: null`. Only the
    /// gym and swim templates carry a `routine:` field; passing
    /// `Some(...)` on `body` or `generic` is allowed but silently
    /// no-ops (the field doesn't exist in those templates).
    ///
    /// Errors:
    /// - [`DomainError::EmptyField`] — `activity` is whitespace-only.
    /// - [`DomainError::MalformedWikilink`] — `routine` is non-empty
    ///   and already contains `[[` or `]]`; pass the bare slug.
    /// - [`StoreError::NotFound`] — no stewardship matches the slug.
    /// - [`DomainError::TrackingOnFlatStewardship`] — slug resolves
    ///   to a flat dashboard (no `tracking/` subdir).
    /// - [`StoreError::AlreadyExists`] — a tracking note with the
    ///   same date and activity already exists (logging the same
    ///   activity twice on the same day should be one merged note,
    ///   not two silently-overwriting writes).
    pub fn add_tracking_entry(
        &self,
        at: NaiveDateTime,
        stewardship: &str,
        activity: &str,
        routine: Option<&str>,
        content: &str,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let activity = activity.trim();
        if activity.is_empty() {
            return Err(DomainError::EmptyField { field: "activity" });
        }
        let activity_slug = slugify(activity);

        let routine = routine.map(str::trim).filter(|s| !s.is_empty());
        if let Some(r) = routine
            && (r.contains("[[") || r.contains("]]"))
        {
            return Err(DomainError::MalformedWikilink {
                value: r.to_owned(),
            });
        }

        let (_dashboard_path, variant) = self.resolve_stewardship_with_variant(stewardship)?;
        if variant != StewardshipVariant::Expanded {
            return Err(DomainError::TrackingOnFlatStewardship(
                stewardship.to_owned(),
            ));
        }

        let date = at.date();
        let filename = format!("{}-{activity_slug}.md", date.format("%Y-%m-%d"));
        let path = VaultPath::new(format!("stewardships/{stewardship}/tracking/{filename}",))?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }

        let body = self.render_tracking(stewardship, &activity_slug, date, routine, content)?;
        let entry = build_index_entry_for(&path, &body, NoteType::Tracking.as_str())?;

        tx.write_file(path.clone(), body);
        tx.upsert_note(entry);
        tx.commit()?;

        Ok(path)
    }
}

impl Vault {
    /// Render the tracking template for `activity_slug` (custom or
    /// built-in). The engine resolves `tracking-<activity>` for the
    /// known activities (gym/body/swim) and falls back to the generic
    /// `tracking` template otherwise. `routine` becomes a quoted
    /// routine wikilink when present, else `null`; only templates with a
    /// `routine:` field (gym) consume it.
    fn render_tracking(
        &self,
        stewardship: &str,
        activity_slug: &str,
        date: NaiveDate,
        routine: Option<&str>,
        content: &str,
    ) -> Result<String, DomainError> {
        let date_long = format!(
            "{day} {month} {year}",
            day = date.day(),
            month = date.format("%B"),
            year = date.year(),
        );
        let routine_yaml = match routine {
            Some(slug) => format!("\"[[stewardships/{stewardship}/routines/{slug}]]\""),
            None => "null".to_owned(),
        };
        let mut ctx = VariableContext::new();
        ctx.set_contextual("stewardship", stewardship);
        ctx.set_contextual("activity", activity_slug);
        ctx.set_contextual("activity_title", title_case(activity_slug));
        ctx.set_contextual("date", date.format("%Y-%m-%d").to_string());
        ctx.set_contextual("date_long", date_long);
        ctx.set_contextual("content", content.trim_end());
        ctx.set_contextual("routine", routine_yaml);
        self.scaffold("tracking", Some(activity_slug), &ctx)
    }
}

/// Crude title-case for the generic template's H1 — capitalises the
/// first character of each `-`-separated word, leaves the rest as-is.
/// Good enough for the most common slugs (`yoga`, `run`, `meditation`);
/// the user can edit the H1 if they want something fancier.
fn title_case(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
