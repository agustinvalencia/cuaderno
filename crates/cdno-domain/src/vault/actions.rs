//! Heavy-form action notes: the manifest form of an action (design
//! §5.11). Most actions stay as inline `- [ ]` bullets in a project's
//! `## Next Actions` section; an action note is the opt-in heavier
//! form for multi-day, multi-evidence work.
//!
//! This module owns the *birth* path — creating a note and attaching
//! it to a project bullet. The *death* path (archiving an attached
//! note when its bullet is completed) lives next to `complete_action`
//! in `projects/actions.rs` but calls [`Vault::stage_action_archival`]
//! here so the move-and-stamp logic sits with the rest of the note
//! lifecycle.

use chrono::{Datelike, NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::path::VaultPath;
use cdno_core::template::VariableContext;
use cdno_core::transaction::VaultTransaction;

use crate::error::DomainError;
use crate::frontmatter::{ActionStatus, EnergyLevel};
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::projects::{NEXT_ACTIONS_SECTION, rewrite_field_in_frontmatter};
use super::slug::slugify;

impl Vault {
    /// Build a new action note at `actions/<slug>.md` and stage its
    /// write + index upsert into the caller's transaction `tx`.
    ///
    /// The caller owns `tx` (and the write lock it holds), opening it
    /// before its own project read so the whole operation — the project
    /// bullet rewrite for `add_action_with_note`, the bullet-promotion
    /// edit for `promote_action` — commits atomically under one lock
    /// (#196). Nothing is written until the caller commits.
    ///
    /// `milestone` is a raw wikilink string (the milestone owns the
    /// date, the action inherits it); `due` is a self-imposed deadline
    /// used only when the action stands alone. Both are `None` for the
    /// plain `add --note` path. Errors with
    /// [`StoreError::AlreadyExists`] if an active action already holds
    /// the slug.
    // The action's defining fields plus the caller's transaction; bundling
    // them into a struct would obscure more than it clarifies.
    #[allow(clippy::too_many_arguments)]
    pub fn create_action_note(
        &self,
        tx: &mut VaultTransaction,
        at: NaiveDateTime,
        project: &str,
        title: &str,
        energy: EnergyLevel,
        milestone: Option<&str>,
        due: Option<NaiveDate>,
    ) -> Result<VaultPath, DomainError> {
        let title = title.trim();
        let slug = slugify(title);
        let path = Self::active_action_path(&slug)?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }

        let milestone_yaml = match milestone {
            Some(m) => format!("\"{m}\""),
            None => "null".to_owned(),
        };
        let due_yaml = due
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "null".to_owned());
        let mut ctx = VariableContext::new();
        ctx.set_contextual("status", ActionStatus::Active.as_str());
        ctx.set_contextual("project", project);
        ctx.set_contextual("energy", energy.as_str());
        ctx.set_contextual("milestone", milestone_yaml);
        ctx.set_contextual("due", due_yaml);
        ctx.set_contextual("created", at.date().format("%Y-%m-%d").to_string());
        ctx.set_contextual("completed", "null");
        ctx.set_contextual("blocker", "null");
        ctx.set_contextual("criteria", "null");
        ctx.set_contextual("tags", "[]");
        ctx.set_contextual("title", title);
        ctx.set_contextual("slug", slug.as_str());
        let content = self.scaffold("action", None, &mut ctx)?;
        let entry = build_index_entry_for(&path, &content, NoteType::Action.as_str())?;

        tx.write_file(path.clone(), content);
        tx.upsert_note(entry);
        Ok(path)
    }

    /// Add an action to a project *and* create its manifest note in a
    /// single transaction. The bullet appended to `## Next Actions`
    /// wikilinks the note — `- [ ] [[actions/<slug>]] (<energy>)` —
    /// rather than carrying the action text inline.
    ///
    /// The parent project is resolved and validated first, so a parked
    /// or missing project fails before any file is staged. Returns the
    /// path of the new action note.
    ///
    /// Errors: parked → `ProjectNotActive`, missing project →
    /// `Store(NotFound)`, slug collision on the note →
    /// `Store(AlreadyExists)`.
    pub fn add_action_with_note(
        &self,
        at: NaiveDateTime,
        project: &str,
        title: &str,
        energy: EnergyLevel,
    ) -> Result<VaultPath, DomainError> {
        // One transaction, opened before any read, so the write lock
        // covers the project's read-modify-write as well as the note
        // creation (#196). The project is validated under the lock; a bad
        // project errors and the uncommitted transaction rolls back.
        let mut tx = self.transaction()?;
        let (project_path, mut doc) = self.resolve_active_project(project)?;
        let note_path = self.create_action_note(&mut tx, at, project, title, energy, None, None)?;
        let action_slug = action_slug_from_path(&note_path);

        // Append the wikilinked bullet, mirroring add_action's section
        // normalisation (single newline before the bullet, trailing
        // blank line before the next heading).
        let bullet = format!(
            "- [ ] [[{}/{action_slug}]] ({})",
            cdno_core::paths::ACTIONS,
            energy.as_str()
        );
        doc.ensure_section(NEXT_ACTIONS_SECTION)?;
        let existing = doc.section(NEXT_ACTIONS_SECTION)?.trim_end();
        let new_section = if existing.is_empty() {
            format!("{bullet}\n\n")
        } else {
            format!("{existing}\n{bullet}\n\n")
        };
        doc.replace_section(NEXT_ACTIONS_SECTION, &new_section)?;
        let new_content = doc.render().to_owned();
        let project_entry =
            build_index_entry_for(&project_path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format!(
            "action added to [[{project}]] with note [[{}/{action_slug}]] — {} ({})",
            cdno_core::paths::ACTIONS,
            title.trim(),
            energy.as_str()
        );

        tx.write_file(project_path, new_content);
        tx.upsert_note(project_entry);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(note_path)
    }

    /// Stage the archival of a completed action note onto an existing
    /// transaction: stamp `status: completed` + `completed: <today>`,
    /// move `actions/<slug>.md` to `actions/_done/<year>/<slug>.md`,
    /// and swap the index rows. Called from `complete_action` when the
    /// completed bullet wikilinks an action note.
    ///
    /// **Drift guard**: a wikilink bullet whose note no longer exists
    /// is not an error — the bullet still completes, there's simply
    /// nothing to archive. The caller's bullet removal is the
    /// meaningful effect in that case.
    ///
    /// Errors with [`StoreError::AlreadyExists`] only if the
    /// destination is already occupied (a same-year slug clash in
    /// `_done`).
    pub(in crate::vault) fn stage_action_archival(
        &self,
        at: NaiveDateTime,
        action_slug: &str,
        tx: &mut VaultTransaction,
    ) -> Result<(), DomainError> {
        let active = Self::active_action_path(action_slug)?;
        if !self.store.exists(&active)? {
            return Ok(());
        }

        let completion = at.date();
        let done_dir = cdno_core::paths::actions_done_dir(completion.year());
        let done = VaultPath::new(format!("{done_dir}/{action_slug}.md"))?;
        if self.store.exists(&done)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                done.to_string(),
            )));
        }

        let raw = self.store.read_file(&active)?;
        let after_status =
            rewrite_field_in_frontmatter(&raw, "status", ActionStatus::Completed.as_str())?;
        let new_content = rewrite_field_in_frontmatter(
            &after_status,
            "completed",
            &completion.format("%Y-%m-%d").to_string(),
        )?;
        let done_entry = build_index_entry_for(&done, &new_content, NoteType::Action.as_str())?;

        // Snapshot the file at archival so the append-only lint (#111)
        // has a baseline to compare against. The whole file is hashed —
        // any future edit to the frontmatter or an existing body line
        // flags; only bytes appended past `frozen_size` are allowed
        // (the design's "six months later, follow-up" case).
        let archived_at_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let snapshot = cdno_core::index::ArchivalSnapshot {
            frozen_size: new_content.len() as u64,
            frozen_hash: cdno_core::hash::content_hash(&new_content),
            archived_at_ns,
        };

        // Write-new + delete-old is a content-changing move; the index
        // swap mirrors complete_commitment.
        tx.write_file(done.clone(), new_content);
        tx.delete_file(active.clone());
        tx.upsert_note(done_entry);
        tx.remove_note(active);
        tx.record_archival_snapshot(done, snapshot);
        Ok(())
    }

    /// Vault-relative path of an active action note: `actions/<slug>.md`.
    fn active_action_path(slug: &str) -> Result<VaultPath, DomainError> {
        VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::ACTIONS))
            .map_err(DomainError::from)
    }
}

/// The slug is the file stem of the note path. `create_action_note`
/// always builds the path from a slug, so the stem is present and
/// valid UTF-8; the fallback keeps a hand-mangled path from panicking.
fn action_slug_from_path(path: &VaultPath) -> &str {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
}
