//! `set_core_question`: change an active project's `core_question:`
//! frontmatter after creation, auto-logging the previous value to
//! today's daily note in a single committed transaction.
//!
//! `core_question` is a first-class project field — it is in
//! `NoteType::Project`'s `frontmatter_order`, typed on
//! `ProjectFrontmatter`, emitted by the template, and resolved into
//! question backlinks by the context layer — but until #523 it could
//! only ever be written once, at creation. `set_frontmatter` refuses
//! it (the settable set comes purely from `[schemas.<type>.fields]`
//! config, and the shipped default declares no schemas), so a project
//! created without a core question, or one whose question changed,
//! could only be fixed by hand-editing the file: the one edit that
//! desyncs the index.
//!
//! Shipped as a dedicated operation rather than a config declaration
//! for three reasons, all recorded on #523 and #430: `cdno init`
//! rewrites `config.toml`, so a declaration-based fix silently
//! regresses on a fresh machine; a dedicated verb keeps
//! `create_project`'s bare-target convention instead of forcing the
//! wrapped `[[questions/foo]]` form a generic field-set would need;
//! and only a dedicated verb carries the auto-logging every other
//! project mutation has.

use chrono::NaiveDateTime;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::WriteOutcome;
use super::super::index_entry::build_index_entry_for;
use super::rewrite_field_in_frontmatter;

impl Vault {
    /// Set (or clear) an active project's `core_question:`.
    ///
    /// `core_question` is the wikilink *target* (e.g.
    /// `"questions/research/foo"`), matching `create_project`'s
    /// parameter exactly — this method wraps it in `[[…]]`. `None`
    /// writes `core_question: null`, which is how a question is
    /// detached from a project.
    ///
    /// Errors:
    /// - [`DomainError::MalformedWikilink`] — the target already
    ///   carries `[[`/`]]`. Surfaced rather than stripped, so a caller
    ///   passing the wrapped form learns the convention instead of
    ///   silently getting a double-wrapped link.
    /// - [`DomainError::ProjectNotActive`] — the project is parked, or
    ///   its frontmatter `status` is not `active`.
    /// - [`StoreError::NotFound`](cdno_core::error::StoreError::NotFound)
    ///   — no such project.
    /// - [`DomainError::MissingFrontmatterField`] — the note has no
    ///   `core_question:` line to rewrite. The shipped template always
    ///   emits one (`null` when unset), so this only reaches a vault
    ///   whose custom project template dropped the key; ordered-insert
    ///   of an absent key is the same follow-up `set_frontmatter`
    ///   tracks, and is deliberately not forked here.
    ///
    /// Setting the value it already holds is a silent no-op — no log
    /// entry, no rewrite — matching `update_project_state`, and
    /// reported through [`WriteOutcome::noop`] so the desktop layer
    /// does not journal an echo-suppression entry for a write that
    /// never happened.
    ///
    /// `at` is a parameter so tests can pin the log timestamp and the
    /// daily-note date; production callers pass
    /// `chrono::Local::now().naive_local()`.
    pub fn set_core_question(
        &self,
        at: NaiveDateTime,
        slug: &str,
        core_question: Option<&str>,
    ) -> Result<WriteOutcome, DomainError> {
        let target = match core_question {
            Some(t) => {
                let t = t.trim();
                if t.is_empty() {
                    return Err(DomainError::EmptyField {
                        field: "core_question",
                    });
                }
                if t.contains("[[") || t.contains("]]") {
                    return Err(DomainError::MalformedWikilink {
                        value: t.to_owned(),
                    });
                }
                Some(t)
            }
            None => None,
        };

        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let (path, doc) = self.resolve_active_project(slug)?;

        // The existing value as it sits in the frontmatter, wikilink
        // wrapping and all — that is what the `was:` line should show,
        // since it is what the file said before this call.
        let previous = doc
            .frontmatter()
            .optional_field::<String>("core_question")?
            .filter(|v| !v.trim().is_empty());

        let new_yaml = match target {
            Some(t) => format!("\"[[{t}]]\""),
            None => "null".to_owned(),
        };
        let new_rendered = target.map(|t| format!("[[{t}]]"));
        if previous.as_deref() == new_rendered.as_deref() {
            return Ok(WriteOutcome::noop(path));
        }

        let raw = self.store.read_file(&path)?;
        let new_content = rewrite_field_in_frontmatter(&raw, "core_question", &new_yaml)?;
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format_core_question_log_entry(slug, previous.as_deref(), &new_rendered);

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        let touched = tx.commit()?;

        Ok(WriteOutcome::written(path, touched))
    }
}

/// Build the daily-log entry recording a core-question change, in the
/// same header-plus-indented-`was:`/`now:` shape
/// `update_project_state` established for the project map's other
/// mutable field. Reusing the shape rather than inventing a third
/// keeps "how did this project change" one search over the daily log.
///
/// The prefix is new, and nothing parses it yet: the context reader
/// keys off `state on [[` and the action prefixes only. It is written
/// for a human reading the log back, and is a stable shape for a
/// future reader to key off — which is why it is built here rather
/// than formatted inline at the call site.
fn format_core_question_log_entry(
    slug: &str,
    previous: Option<&str>,
    new: &Option<String>,
) -> String {
    fn render(value: Option<&str>) -> &str {
        value.unwrap_or(NO_CORE_QUESTION)
    }
    format!(
        "{LOG_CORE_QUESTION_PREFIX}[[{slug}]]\n  was: {}\n  now: {}",
        render(previous),
        render(new.as_deref()),
    )
}

/// Log prefix for a core-question change. A named constant rather than
/// a repeated literal so the writer and any future reader cannot
/// drift — the failure #453 was.
const LOG_CORE_QUESTION_PREFIX: &str = "core question on ";

/// Rendered stand-in for an absent core question on either side of the
/// change, so `was:`/`now:` always carry a value and the entry reads
/// the same whether a question was set, changed, or detached.
const NO_CORE_QUESTION: &str = "(none)";
