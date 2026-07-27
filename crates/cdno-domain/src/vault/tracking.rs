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
//! Only the neutral `generic` template ships built-in; activity-specific
//! variants are per-vault — a `.cuaderno/templates/tracking-<activity>.md`
//! file is picked up automatically, else the activity falls back to the
//! generic template. The user fleshes out the table or notes after the file
//! is created — this op writes the scaffold and gets out of the way.
//! (Ready-made variants live in `examples/templates/tracking/`.)

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate, NaiveDateTime};

use cdno_core::frontmatter::Frontmatter;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;
use cdno_core::template::{TemplateSource, VariableContext};

use crate::error::DomainError;
use crate::frontmatter::TrackingFrontmatter;
use crate::note_type::NoteType;

use super::Vault;
use super::frontmatter_edit::merge_fields_into_frontmatter;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;
use super::stewardships::StewardshipVariant;
use super::write_outcome::WriteOutcome;

/// How far from today a tracking entry may be dated. Backdating is the point
/// (#482) — spending is reconciled from a statement days later, a balance is
/// read whenever the app happens to be open — but an unbounded caller-supplied
/// date combined with agent-written content means history is writable, and a
/// mistaken or injected call can place points that silently reshape a trend.
/// Fifty years back covers any realistic import; a year forward absorbs a
/// deliberate future entry without admitting the mistyped `2062`.
const BACKFILL_YEARS: i32 = 50;
const LOOKAHEAD_YEARS: i32 = 1;

/// The inputs for one tracking entry.
///
/// A params struct rather than more positional arguments: the write already
/// carried six and `#[allow(clippy::too_many_arguments)]` with it, and the
/// date and metrics would have made eight. Build one with
/// [`new`](Self::new) and chain only the parts that apply.
#[derive(Debug, Clone, Default)]
pub struct TrackingEntryDraft {
    /// Slug of the expanded stewardship the entry files under.
    pub stewardship: String,
    /// The activity, which also selects the template variant.
    pub activity: String,
    /// Bare slug of a routine doc; the domain wraps the wikilink.
    pub routine: Option<String>,
    /// Body of the entry's `## Notes` section.
    pub content: String,
    /// Values for the template's prompted variables (`[variables.prompt]`).
    pub prompted: HashMap<String, String>,
    /// The date the tracked thing happened. `None` means the write's `now`.
    pub date: Option<NaiveDate>,
    /// Structured metrics merged into the entry's frontmatter. Scalar values
    /// are type-checked against `[schemas.tracking.fields]` where the vault
    /// declares them; nested values (a record sequence) are written as given,
    /// since the schema grammar cannot declare a list yet.
    pub metrics: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TrackingEntryDraft {
    /// A draft with only the two required parts set.
    pub fn new(stewardship: impl Into<String>, activity: impl Into<String>) -> Self {
        Self {
            stewardship: stewardship.into(),
            activity: activity.into(),
            ..Self::default()
        }
    }

    /// Set the `## Notes` body.
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    /// Set the routine wikilink target (a bare slug).
    pub fn with_routine(mut self, routine: impl Into<String>) -> Self {
        self.routine = Some(routine.into());
        self
    }

    /// Supply prompted-variable values for the resolved template.
    pub fn with_prompted(mut self, prompted: HashMap<String, String>) -> Self {
        self.prompted = prompted;
        self
    }

    /// Date the entry at `date` rather than the write's `now`.
    pub fn on(mut self, date: NaiveDate) -> Self {
        self.date = Some(date);
        self
    }

    /// Merge `metrics` into the entry's frontmatter.
    pub fn with_metrics(mut self, metrics: serde_json::Map<String, serde_json::Value>) -> Self {
        self.metrics = Some(metrics);
        self
    }
}

impl Vault {
    /// File a tracking note under an expanded stewardship.
    ///
    /// The path is
    /// `stewardships/<stewardship>/tracking/<YYYY-MM-DD>-<activity-slug>.md`.
    /// The activity slug selects the template: a vault's
    /// `.cuaderno/templates/tracking-<slug>.md` if present, else the built-in
    /// generic template (an empty Notes section the user fills in). No
    /// activity-specific templates ship built-in.
    ///
    /// `content` becomes the body of the `## Notes` section. Pass
    /// `""` to leave it blank — the file is intended to be edited
    /// after creation.
    ///
    /// `routine` is an optional bare wikilink target (e.g.
    /// `"upper-body-a"`) that the domain wraps into
    /// `[[stewardships/<stewardship>/routines/<routine>]]` and
    /// substitutes for a template's `routine: null`. It only takes effect
    /// when the resolved template carries a `routine:` field (e.g. the
    /// example gym/swim variants); on a template without one — including the
    /// generic default — passing `Some(...)` is allowed but silently no-ops.
    ///
    /// Errors:
    /// - [`DomainError::EmptyField`] — `activity` is whitespace-only.
    /// - [`DomainError::MalformedWikilink`] — `routine` is non-empty
    ///   and already contains `[[` or `]]`; pass the bare slug.
    /// - [`StoreError::NotFound`] — no stewardship matches the slug.
    /// - [`DomainError::TrackingOnFlatStewardship`] — slug resolves
    ///   to a flat dashboard (no `tracking/` subdir).
    ///
    /// A second entry for the same `(activity, date)` **merges** into the
    /// first (`#488`) rather than erroring: its `content` is appended to
    /// `## Notes` and its metrics are folded into the frontmatter. The
    /// returned [`TemplateSource`] is `None` for a merge, since no template
    /// is resolved.
    pub fn add_tracking_entry(
        &self,
        now: NaiveDateTime,
        draft: TrackingEntryDraft,
    ) -> Result<(WriteOutcome, Option<TemplateSource>), DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let activity = draft.activity.trim();
        if activity.is_empty() {
            return Err(DomainError::EmptyField { field: "activity" });
        }
        let activity_slug = slugify(activity);
        let stewardship = draft.stewardship.as_str();

        let routine = draft
            .routine
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(r) = routine
            && (r.contains("[[") || r.contains("]]"))
        {
            return Err(DomainError::MalformedWikilink {
                value: r.to_owned(),
            });
        }

        // The entry's date defaults to the write's own clock; an explicit one
        // is bounded, because it is the parameter that lets a caller rewrite
        // history.
        let date = draft.date.unwrap_or_else(|| now.date());
        if draft.date.is_some() {
            check_plausible_date(date, now.date())?;
        }

        let (_dashboard_path, variant) = self.resolve_stewardship_with_variant(stewardship)?;
        if variant != StewardshipVariant::Expanded {
            return Err(DomainError::TrackingOnFlatStewardship(
                stewardship.to_owned(),
            ));
        }

        let filename = format!("{}-{activity_slug}.md", date.format("%Y-%m-%d"));
        let path = VaultPath::new(format!("stewardships/{stewardship}/tracking/{filename}",))?;

        // A second entry for the same (activity, date) merges into the first
        // rather than erroring (#488). Several domains are naturally
        // multi-occurrence — spending happens throughout a day, contact more
        // than once, practice splits morning and evening — and for
        // agent-driven logging a day recorded in two passes is ordinary.
        if self.store.exists(&path)? {
            if let Some(metrics) = &draft.metrics {
                check_no_identity_metrics(metrics)?;
                self.check_declared_metrics(metrics)?;
            }
            let existing = self.store.read_file(&path)?;
            let merged = merge_into_entry(&existing, &draft)?;
            let entry = build_index_entry_for(&path, &merged, NoteType::Tracking.as_str())?;
            tx.write_file(path.clone(), merged);
            tx.upsert_note(entry);
            let log_entry = format_tracking_log_entry(&path, &activity_slug, date, now.date());
            self.stage_daily_log(now, &log_entry, &mut tx)?;
            let touched = tx.commit()?;
            // No template is resolved on a merge - the note already exists
            // and nothing is rendered - so report none rather than inventing
            // a rung. Re-rendering just to learn it could also fail on
            // unresolved prompts, for a render whose output is discarded.
            return Ok((WriteOutcome::written(path, touched), None));
        }

        let (body, source) = self.render_tracking(
            stewardship,
            &activity_slug,
            date,
            routine,
            &draft.content,
            &draft.prompted,
        )?;

        // Metrics land in the rendered note's frontmatter, validated against
        // whatever `[schemas.tracking.fields]` declares. The merge is a
        // separate step from rendering because a template is pure variable
        // substitution: it has no way to carry a record sequence.
        let body = match &draft.metrics {
            Some(metrics) if !metrics.is_empty() => {
                check_no_identity_metrics(metrics)?;
                self.check_declared_metrics(metrics)?;
                let merged = merge_fields_into_frontmatter(&body, metrics)?;
                // The invariant, not just the denylist: whatever the merge
                // produced must still parse as a tracking note. Committing one
                // that does not would not merely corrupt this entry — every
                // reader that scans all tracking notes (`list_tracking`,
                // `list_stewardships`) parses before it filters, so one bad
                // note fails the read for every stewardship in the vault.
                let (fm, _rest) = Frontmatter::parse(&merged)?;
                TrackingFrontmatter::try_from(fm)?;
                merged
            }
            _ => body,
        };

        let entry = build_index_entry_for(&path, &body, NoteType::Tracking.as_str())?;

        tx.write_file(path.clone(), body);
        tx.upsert_note(entry);

        // Audit line in TODAY's log, not the entry's day. A backdated write is
        // exactly the one worth being able to find later, and journalling it
        // into the day it claims to describe would both hide it and scaffold a
        // daily note for a day that never had one.
        let log_entry = format_tracking_log_entry(&path, &activity_slug, date, now.date());
        self.stage_daily_log(now, &log_entry, &mut tx)?;

        let touched = tx.commit()?;
        Ok((WriteOutcome::written(path, touched), Some(source)))
    }

    /// Type-check the scalar metrics a caller supplied against the vault's
    /// `[schemas.tracking.fields]` declarations, using the same
    /// [`FieldSpec::check_value`](cdno_core::config::FieldSpec::check_value)
    /// the lint layer applies to a note already on disk.
    ///
    /// Undeclared keys pass: undeclared frontmatter is legal everywhere else
    /// in the vault, and `[tracking.<activity>]` declares how a metric
    /// *reduces* rather than which keys an entry may carry. Nested values pass
    /// too — the schema grammar has
    /// no list shape (`list = true` is still a load error), so a record
    /// sequence has nothing to check against.
    fn check_declared_metrics(
        &self,
        metrics: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), DomainError> {
        // Gated on an explicit `fields` block, exactly as the lint value-check
        // is (`lint.rs`). Without the gate, a legacy `extra_required` entry —
        // which desugars to an untyped *string* spec and is documented as
        // lint-only — would become a hard create-time rejection, so a vault
        // that merely lists `weight` as required could not write `weight: 82.5`
        // at all.
        let declared = self
            .config
            .schema_for(NoteType::Tracking.as_str())
            .filter(|s| !s.fields.is_empty())
            .map(|s| s.declared_fields())
            .unwrap_or_default();
        for (key, value) in metrics {
            if value.is_null() || value.is_array() || value.is_object() {
                continue;
            }
            if let Some(spec) = declared.get(key.as_str())
                && let Some(reason) = spec.check_value(value)
            {
                return Err(DomainError::InvalidFieldValue {
                    note_type: NoteType::Tracking.as_str().to_owned(),
                    field: key.clone(),
                    reason,
                });
            }
        }
        Ok(())
    }
}

/// Fold a draft into an existing entry for the same `(activity, date)`
/// (`#488`): its content appended to `## Notes`, its metrics merged into the
/// frontmatter.
///
/// Merging is **not** blind concatenation. The `AlreadyExists` guard this
/// replaces was the only thing preventing a double-write, so appending
/// unconditionally would make re-running an import append the same records
/// again and double-count every `sum` metric — and a reconciled domain, where
/// re-runs are ordinary, is exactly where that bites.
///
/// - **A record carrying an `id` replaces the record with that `id`**, so
///   re-applying an identical payload is idempotent.
/// - **A record without an `id` appends**, and re-running double-counts. An
///   import path should supply one.
/// - **A scalar is last-write-wins**: there is no array to key on.
///
/// Records append in order, so document order — which is what `last` reduces
/// on — stays well defined across a merged day.
fn merge_into_entry(existing: &str, draft: &TrackingEntryDraft) -> Result<String, DomainError> {
    let (fm, _body) = Frontmatter::parse(existing)?;
    let current = fm.as_json();

    let mut merged = serde_json::Map::new();
    if let Some(metrics) = &draft.metrics {
        for (key, incoming) in metrics {
            let value = match (current.get(key), incoming) {
                // Two record sets: merge on identity.
                (Some(serde_json::Value::Array(old)), serde_json::Value::Array(new)) => {
                    serde_json::Value::Array(merge_records(old, new))
                }
                // Anything else - a scalar, or a shape change - is replaced.
                _ => incoming.clone(),
            };
            merged.insert(key.clone(), value);
        }
    }

    let out = if merged.is_empty() {
        existing.to_owned()
    } else {
        merge_fields_into_frontmatter(existing, &merged)?
    };

    let content = draft.content.trim_end();
    if content.is_empty() {
        return Ok(out);
    }
    // Tracking notes are append-only: the second pass adds to the record, it
    // does not replace it.
    let mut doc = MarkdownDocument::parse(out)?;
    doc.ensure_section(TRACKING_NOTES_SECTION)?;
    doc.append_to_section(TRACKING_NOTES_SECTION, &format!("{content}\n"))?;
    Ok(doc.render().to_owned())
}

/// Merge two record sequences on each record's optional `id`.
///
/// A record whose `id` matches one already present replaces it in place,
/// keeping its position; everything else appends in order.
fn merge_records(old: &[serde_json::Value], new: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let id_of = |record: &serde_json::Value| -> Option<String> {
        record.get("id").and_then(|id| match id {
            serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
    };

    let mut out = old.to_vec();
    for record in new {
        match id_of(record) {
            Some(id) => match out.iter().position(|r| id_of(r).as_deref() == Some(&id)) {
                Some(at) => out[at] = record.clone(),
                None => out.push(record.clone()),
            },
            None => out.push(record.clone()),
        }
    }
    out
}

/// The heading a tracking entry's prose lives under.
const TRACKING_NOTES_SECTION: &str = "Notes";

/// The frontmatter keys a tracking note is *identified* by: its type marker
/// and the three fields `TrackingFrontmatter` requires. A metric may not name
/// one of these.
///
/// They are the note's identity, not data about it, and the engine owns every
/// one: `type` decides which queries see the note at all (and is re-derived
/// from frontmatter on the next reconcile), `stewardship` and `activity` are
/// what readers group by, and `date` is fixed by the filename — a metric
/// rewriting it would leave the two disagreeing permanently.
///
/// `duration_min` and `routine` are deliberately absent: both are ordinary
/// optional fields, and a duration is a perfectly good metric.
///
/// `set_frontmatter` refuses this same class through its own default-deny
/// (declared *and* `settable = true`, then type-checked); this path needs its
/// own guard because a metric key is caller-supplied and otherwise unchecked.
const IDENTITY_KEYS: &[&str] = &["type", "stewardship", "activity", "date"];

/// Reject a metrics payload naming a key that identifies the note.
fn check_no_identity_metrics(
    metrics: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), DomainError> {
    for key in metrics.keys() {
        if IDENTITY_KEYS.contains(&key.as_str()) {
            return Err(DomainError::ReservedSchemaField {
                note_type: NoteType::Tracking.as_str().to_owned(),
                field: key.clone(),
            });
        }
    }
    Ok(())
}

/// Reject a date far enough from today that it is almost certainly a typo.
/// See [`BACKFILL_YEARS`] for why the bound exists at all.
fn check_plausible_date(date: NaiveDate, today: NaiveDate) -> Result<(), DomainError> {
    // `with_year` fails only on 29 February; stepping back a day first keeps
    // the bound well-defined without pulling in a calendar-arithmetic crate.
    let shift = |years: i32| -> NaiveDate {
        let anchor = if today.month() == 2 && today.day() == 29 {
            today - chrono::Duration::days(1)
        } else {
            today
        };
        anchor
            .with_year(anchor.year() + years)
            .unwrap_or(chrono::NaiveDate::MAX)
    };
    let earliest = shift(-BACKFILL_YEARS);
    let latest = shift(LOOKAHEAD_YEARS);
    if date < earliest || date > latest {
        return Err(DomainError::ImplausibleDate {
            date,
            earliest,
            latest,
        });
    }
    Ok(())
}

/// The daily-log line recording a filed tracking entry. A backdated entry
/// names the day it describes, so the log reads as "filed today, about then"
/// rather than silently claiming the session happened now.
fn format_tracking_log_entry(
    path: &VaultPath,
    activity: &str,
    date: NaiveDate,
    today: NaiveDate,
) -> String {
    let link = path.to_string();
    let link = link.strip_suffix(".md").unwrap_or(&link);
    if date == today {
        format!("Tracked {activity}: [[{link}]]")
    } else {
        format!("Tracked {activity} for {date}: [[{link}]]")
    }
}

impl Vault {
    /// Render the tracking template for `activity_slug` (custom or
    /// built-in). The engine resolves `tracking-<activity>` for the
    /// a vault-provided `tracking-<activity>` template if present and falls
    /// back to the generic `tracking` template otherwise. `routine` becomes a
    /// quoted routine wikilink when present, else `null`; only templates with a
    /// `routine:` field consume it.
    #[allow(clippy::too_many_arguments)] // thin gather→render passthrough
    fn render_tracking(
        &self,
        stewardship: &str,
        activity_slug: &str,
        date: NaiveDate,
        routine: Option<&str>,
        content: &str,
        prompted: &HashMap<String, String>,
    ) -> Result<(String, TemplateSource), DomainError> {
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
        for (k, v) in prompted {
            ctx.set_prompted(k, v);
        }
        self.scaffold_with_source("tracking", Some(activity_slug), &mut ctx)
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
