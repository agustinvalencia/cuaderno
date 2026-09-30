//! `update_project_state`: rewrites the `## Current State` section
//! of an active project, auto-logging the previous body to today's
//! daily note in a single committed transaction.

use chrono::NaiveDateTime;

use cdno_core::config::StateOverflow;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::WriteOutcome;
use super::super::index_entry::build_index_entry_for;
use super::super::log::flatten_for_log;
use super::CURRENT_STATE_SECTION;

impl Vault {
    /// Replace an active project's `Current State` section, auto-logging
    /// the previous state to today's daily note in a single committed
    /// transaction.
    ///
    /// `slug` identifies the project (matching the CLI surface,
    /// `cdno project state <slug> "..."`), located on disk across
    /// `projects/`, `projects/_parked/` and `projects/_done/<year>/`.
    /// Resolves errors as:
    /// - the project is not both directly at `projects/<slug>.md` **and**
    ///   `status: active` in its frontmatter (a parked or closed project,
    ///   or a hand-edited map whose folder and status disagree) →
    ///   [`DomainError::ProjectNotActive`]. Folder and frontmatter must
    ///   both say active; neither alone is enough.
    /// - the slug exists at more than one location →
    ///   [`DomainError::AmbiguousProject`].
    /// - file at no location → [`StoreError::NotFound`](cdno_core::error::StoreError::NotFound).
    ///
    /// When `new_state.trim()` equals the existing trimmed state, the
    /// call is a silent no-op — no log entry, no project rewrite —
    /// because logging "was X, now X" is just noise. The returned
    /// [`WriteOutcome`] reports the no-op via `touched() == false` (its
    /// `paths` empty), so the desktop layer skips journalling and its
    /// self-change emit rather than planting a false echo-suppression
    /// entry over paths nothing was written to (#315).
    ///
    /// On a real write, `primary` is the project map and `paths` carries
    /// both it and the daily-log note the previous state was logged to.
    ///
    /// `at` is taken as a parameter so tests can pin the log timestamp
    /// and the daily-note date; production callers pass
    /// `chrono::Local::now().naive_local()`.
    pub fn update_project_state(
        &self,
        at: NaiveDateTime,
        slug: &str,
        new_state: &str,
    ) -> Result<WriteOutcome, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        // Resolved from the disk: parked, closed and misfiled maps are
        // `ProjectNotActive`, a missing slug is `Store(NotFound)`, and
        // folder and frontmatter must both say active.
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let old_state = doc.section(CURRENT_STATE_SECTION)?.trim().to_owned();
        let new_trimmed = new_state.trim();
        if old_state == new_trimmed {
            // Silent no-op: report the resolved path but signal (empty
            // `paths`) that nothing was written, so the caller doesn't
            // journal or emit for a write that never happened.
            //
            // Ordered *before* the length check on purpose: re-submitting
            // an already-over-limit body verbatim is a no-op, not a
            // rejection — grandfathered content is never retroactively
            // blocked, only a genuine *change* is measured.
            return Ok(WriteOutcome::noop(path));
        }

        // Length ceiling on the Current State snapshot. The old body is
        // auto-logged to the daily just below, so the long-form history
        // survives regardless — capping here only stops agent-driven
        // updates from sprawling into noise. Disabled by `cap == 0` or
        // `state_overflow = "off"`. Char count is Unicode scalars,
        // matching the slug cap.
        let mut warnings = Vec::new();
        let cap = self.config.vault.max_state_chars as usize;
        if cap > 0 && self.config.vault.state_overflow != StateOverflow::Off {
            let len = new_trimmed.chars().count();
            if len > cap {
                // Improvement is always allowed. If the previous body was
                // already over the cap — it predates this setting, or a
                // lowered cap — accept any change that doesn't grow it, so
                // an over-limit state can be trimmed toward compliance
                // across several edits instead of demanding a single edit
                // under the limit. Only a *new* overflow, or growing an
                // existing one, is what `reject` blocks.
                let old_len = old_state.chars().count();
                let shrinking_existing = old_len > cap && len <= old_len;
                if self.config.vault.state_overflow == StateOverflow::Reject && !shrinking_existing
                {
                    return Err(DomainError::StateTooLong {
                        slug: slug.to_owned(),
                        chars: len,
                        max: cap,
                    });
                }
                warnings.push(if shrinking_existing {
                    format!(
                        "Current State for '{slug}' is still {len} characters (over the {cap} \
                         limit), accepted because it's shorter than before \u{2014} keep trimming."
                    )
                } else {
                    format!(
                        "Current State for '{slug}' is {len} characters (over the {cap} limit) \
                         \u{2014} consider trimming; the detail belongs in the daily log."
                    )
                });
            }
        }

        // Normalise so the section ends with a blank line — preserves
        // readability between Current State and the next heading even
        // when the caller passes unterminated prose.
        let normalised_section = format!("{new_trimmed}\n\n");
        doc.replace_section(CURRENT_STATE_SECTION, &normalised_section)?;
        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format_state_change_log_entry(slug, &old_state, new_trimmed);

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        let touched = tx.commit()?;

        Ok(WriteOutcome::written(path, touched).with_warnings(warnings))
    }
}

/// Build the daily-log entry recording a state change. The entry
/// becomes the body of one bullet under `## Logs`: a header line
/// identifying the project, then indented `was:` / `now:`
/// continuation lines so multiline state bodies survive without
/// breaking the line-oriented log format. Whitespace runs in
/// `old_state` and `new_state` (including newlines) collapse to
/// single spaces so each becomes one log line.
fn format_state_change_log_entry(slug: &str, old_state: &str, new_state: &str) -> String {
    format!(
        "state on [[{slug}]]\n  was: {}\n  now: {}",
        flatten_for_log(old_state),
        flatten_for_log(new_state),
    )
}
