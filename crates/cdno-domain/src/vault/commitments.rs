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

use chrono::{Datelike, NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{CommitmentFrontmatter, CommitmentStatus, Context};
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::projects::rewrite_field_in_frontmatter;
use super::slug::slugify;

const COMMITMENT_TEMPLATE: &str = include_str!("../../templates/commitment.md");

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
            return Err(DomainError::Store(StoreError::NotFound(
                active_path.to_string(),
            )));
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
