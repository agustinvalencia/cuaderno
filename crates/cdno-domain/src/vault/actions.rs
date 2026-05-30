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
use cdno_core::transaction::VaultTransaction;

use crate::error::DomainError;
use crate::frontmatter::{ActionStatus, EnergyLevel};
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::projects::{NEXT_ACTIONS_SECTION, rewrite_field_in_frontmatter};
use super::slug::slugify;

const ACTION_TEMPLATE: &str = include_str!("../../templates/action.md");

impl Vault {
    /// Build a new action note at `actions/<slug>.md` and stage its
    /// write + index upsert into a fresh, **uncommitted** transaction.
    ///
    /// Returns the note path and the transaction so callers can add
    /// their own operations (the project bullet rewrite for
    /// `add_action_with_note`, the bullet-promotion edit for a future
    /// `promote_action`) and commit everything atomically as one unit.
    /// Nothing is written until the returned transaction is committed.
    ///
    /// `milestone` is a raw wikilink string (the milestone owns the
    /// date, the action inherits it); `due` is a self-imposed deadline
    /// used only when the action stands alone. Both are `None` for the
    /// plain `add --note` path. Errors with
    /// [`StoreError::AlreadyExists`] if an active action already holds
    /// the slug.
    pub fn create_action_note(
        &self,
        at: NaiveDateTime,
        project: &str,
        title: &str,
        energy: EnergyLevel,
        milestone: Option<&str>,
        due: Option<NaiveDate>,
    ) -> Result<(VaultPath, VaultTransaction), DomainError> {
        let title = title.trim();
        let slug = slugify(title);
        let path = Self::active_action_path(&slug)?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }

        let content = render_action_template(ActionTemplateArgs {
            title,
            slug: &slug,
            project,
            energy,
            milestone,
            due,
            created: at.date(),
        });
        let entry = build_index_entry_for(&path, &content, NoteType::Action.as_str())?;

        let mut tx = self.transaction();
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry);
        Ok((path, tx))
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
        // Validate the parent project up front. Resolving here (rather
        // than inside create_action_note) keeps the note builder
        // project-agnostic and means a bad project never leaves a
        // half-built transaction lying around.
        let (project_path, mut doc) = self.resolve_active_project(project)?;

        let (note_path, mut tx) =
            self.create_action_note(at, project, title, energy, None, None)?;
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

/// Arguments for [`render_action_template`]. Grouped into a struct to
/// keep the renderer's signature readable and the call site explicit.
struct ActionTemplateArgs<'a> {
    title: &'a str,
    slug: &'a str,
    project: &'a str,
    energy: EnergyLevel,
    milestone: Option<&'a str>,
    due: Option<NaiveDate>,
    created: NaiveDate,
}

/// Render the built-in action template with every field stamped. A
/// fresh note is always `active` with no `completed`, `blocker`, or
/// `criteria`. A milestone wikilink is quoted because the unquoted
/// `[[...]]` would parse as a YAML flow sequence; absent optionals
/// render as `null`, and an empty tag list as `[]`.
fn render_action_template(args: ActionTemplateArgs<'_>) -> String {
    let milestone = match args.milestone {
        Some(m) => format!("\"{m}\""),
        None => "null".to_owned(),
    };
    let due = args
        .due
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "null".to_owned());

    ACTION_TEMPLATE
        .replace("{{status}}", ActionStatus::Active.as_str())
        .replace("{{project}}", args.project)
        .replace("{{energy}}", args.energy.as_str())
        .replace("{{milestone}}", &milestone)
        .replace("{{due}}", &due)
        .replace("{{created}}", &args.created.format("%Y-%m-%d").to_string())
        .replace("{{completed}}", "null")
        .replace("{{blocker}}", "null")
        .replace("{{criteria}}", "null")
        .replace("{{tags}}", "[]")
        .replace("{{title}}", args.title)
        .replace("{{slug}}", args.slug)
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
