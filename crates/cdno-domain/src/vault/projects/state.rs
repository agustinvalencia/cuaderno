//! `update_project_state`: rewrites the `## Current State` section
//! of an active project, auto-logging the previous body to today's
//! daily note in a single committed transaction.

use chrono::NaiveDateTime;

use cdno_core::config::StateOverflow;
use cdno_core::error::StoreError;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::WriteOutcome;
use super::super::index_entry::build_index_entry_for;
use super::CURRENT_STATE_SECTION;

impl Vault {
    /// Replace an active project's `Current State` section, auto-logging
    /// the previous state to today's daily note in a single committed
    /// transaction.
    ///
    /// `slug` identifies the project (matching the CLI surface,
    /// `cdno project state <slug> "..."`). Lookup is unambiguous
    /// because slug uniqueness spans `projects/` and
    /// `projects/_parked/`. Resolves errors as:
    /// - file at `projects/_parked/<slug>.md` (parked) or frontmatter
    ///   `status` not `active` → [`DomainError::ProjectNotActive`].
    ///   Folder and frontmatter are checked independently because the
    ///   frontmatter is the source of truth — manual edits could put a
    ///   non-active project under `projects/`.
    /// - file at neither location → [`StoreError::NotFound`].
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
        let active_path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))?;
        let parked_path =
            VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))?;

        let path = if self.store.exists(&active_path)? {
            active_path
        } else if self.store.exists(&parked_path)? {
            return Err(DomainError::ProjectNotActive(slug.to_owned()));
        } else {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{active_path}{}",
                self.available_projects_hint()
            ))));
        };

        let raw = self.store.read_file(&path)?;
        let mut doc = MarkdownDocument::parse(raw)?;
        // Defensive frontmatter check: the file lives under projects/
        // but a manual edit could have set status to parked or
        // completed. Trust the frontmatter, not the folder.
        let project = ProjectFrontmatter::try_from(doc.frontmatter().clone())?;
        if project.status != ProjectStatus::Active {
            return Err(DomainError::ProjectNotActive(slug.to_owned()));
        }

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
        // `state_overflow = "off"`; `reject` blocks, `warn` writes but
        // advises. Char count is Unicode scalars, matching the slug cap.
        let mut warnings = Vec::new();
        let cap = self.config.vault.max_state_chars as usize;
        if cap > 0 && self.config.vault.state_overflow != StateOverflow::Off {
            let len = new_trimmed.chars().count();
            if len > cap {
                match self.config.vault.state_overflow {
                    StateOverflow::Reject => {
                        return Err(DomainError::StateTooLong {
                            slug: slug.to_owned(),
                            chars: len,
                            max: cap,
                        });
                    }
                    StateOverflow::Warn => warnings.push(format!(
                        "Current State for '{slug}' is {len} characters (over the {cap} \
                         limit) \u{2014} consider trimming; the detail belongs in the daily log."
                    )),
                    StateOverflow::Off => unreachable!("guarded by the `!= Off` check above"),
                }
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

fn flatten_for_log(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
