//! `add_action` / `complete_action`: mutate the `## Next Actions`
//! checklist of an active project, with daily-note logging on both
//! the addition (planning trace) and the completion.

use std::collections::HashMap;

use chrono::NaiveDateTime;

use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{ActionFrontmatter, ActionStatus, EnergyLevel};
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::WriteOutcome;
use super::super::index_entry::build_index_entry_for;
use super::NEXT_ACTIONS_SECTION;

/// One open action bullet from a project's `## Next Actions` section,
/// produced by [`Vault::list_actions`]. Closed (`- [x]`) bullets are
/// not part of the action surface — action completion removes the
/// bullet rather than checking it — so the list only carries open
/// items.
// `--json` serialises this directly; field shape + enum casing match the
// MCP `ActionListEntryDto` (energy/status are kebab-case via their serde
// rename, same as the DTO's `as_str()` strings).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub struct ActionListEntry {
    /// The bullet text after `- [ ] `, including any `(<energy>)`
    /// suffix and wikilink target. Preserved verbatim so the caller
    /// can render or re-match against it without re-parsing structure.
    pub text: String,
    /// Energy bucket parsed from the trailing `(deep|medium|light)`
    /// suffix; `None` when the bullet has no recognised suffix.
    pub energy: Option<EnergyLevel>,
    /// `Some` when the bullet wikilinks an action note (`[[actions/
    /// <slug>]]`) **and** that note still exists. A wikilink whose
    /// note is missing surfaces as `None` (the bullet text still
    /// carries the wikilink, signalling drift).
    pub attached: Option<AttachedAction>,
}

/// The action note hanging off a wikilink bullet. Carries the slug
/// and the current frontmatter `status` so a list view can flag
/// active / blocked / completed inline.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub struct AttachedAction {
    pub slug: String,
    pub status: ActionStatus,
}

impl Vault {
    /// Record that work on an action is starting: one line in today's
    /// daily log, `started [[<slug>]] — <action>`.
    ///
    /// This is the single home of the "started" log format — CLI,
    /// MCP, and desktop-Start-button surfaces are expected to call
    /// this rather than compose their own line, so the trace stays
    /// greppable. The
    /// project is resolved first (active projects only) so the logged
    /// wikilink can't dangle; `action` is free text — typically the
    /// bullet the caller picked from `list_actions`, but starting
    /// unplanned work is equally valid.
    ///
    /// Returns the daily-note path touched. Errors mirror the other
    /// action ops: parked → `ProjectNotActive`, missing →
    /// `Store(NotFound)`, whitespace-only action → `EmptyField`.
    pub fn start_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        action: &str,
    ) -> Result<VaultPath, DomainError> {
        let action_text = action.trim();
        if action_text.is_empty() {
            return Err(DomainError::EmptyField { field: "action" });
        }
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        self.resolve_active_project(slug)?;

        let log_entry = format_action_started_log_entry(slug, action_text);
        let daily_path = self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;
        Ok(daily_path)
    }

    /// Append a next action to an active project, also recording the
    /// addition in today's daily log so a planning session leaves a
    /// trace.
    ///
    /// The new line takes the form `- [ ] <action> (<energy>)`, placed
    /// at the end of the `## Next Actions` section. Section formatting
    /// is normalised — a single newline separates the new bullet from
    /// the previous content, and the section ends with a blank line so
    /// the next heading stays cleanly separated.
    ///
    /// Errors mirror `update_project_state`: parked → `ProjectNotActive`,
    /// missing → `Store(NotFound)`, missing section → `Manipulation`.
    pub fn add_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        action: &str,
        energy: EnergyLevel,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let action_text = action.trim();
        let bullet = format!("- [ ] {action_text} ({})", energy.as_str());

        // Auto-create the section if a drifted project is missing it
        // (migration imports, hand-edited files). The user's intent on
        // "add an action" is unambiguous; refusing would force them to
        // edit the file by hand first.
        doc.ensure_section(NEXT_ACTIONS_SECTION)?;
        let existing = doc.section(NEXT_ACTIONS_SECTION)?.trim_end();
        let new_section = if existing.is_empty() {
            format!("{bullet}\n\n")
        } else {
            format!("{existing}\n{bullet}\n\n")
        };
        doc.replace_section(NEXT_ACTIONS_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format_action_added_log_entry(slug, action_text, energy);

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Remove an open action from an active project, logging the
    /// completion to today's daily note. Closed `- [x]` lines are
    /// ignored — only `- [ ]` bullets are candidates, because a
    /// closed line was already manually checked and shouldn't be
    /// silently swept away by a substring query.
    ///
    /// `query` is matched case-insensitively as a substring against
    /// each open action's text (the `(<energy>)` suffix is stripped
    /// before matching). Zero matches → `ActionNotFound`. More than
    /// one match → `AmbiguousAction` carrying the candidate texts so
    /// the user can re-query with enough context to disambiguate.
    ///
    /// Returns a [`WriteOutcome`]: `primary` is the project map, and
    /// `paths` carries every file the commit wrote — the map, the
    /// daily-log note, and (when the bullet wikilinked an action note)
    /// the archival move's source and destination. The desktop layer
    /// journals that full set so the watcher can't echo the archive
    /// writes back as external edits (#315).
    pub fn complete_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
    ) -> Result<WriteOutcome, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let section = doc.section(NEXT_ACTIONS_SECTION)?;
        let lines: Vec<&str> = section.split('\n').collect();
        let removed_idx = resolve_open_action(&lines, slug, query)?;
        let removed_full_text = parse_open_action_text(lines[removed_idx])
            .expect("matched line was previously parseable")
            .to_owned();

        let kept: Vec<&str> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| if i == removed_idx { None } else { Some(*l) })
            .collect();
        let new_section = kept.join("\n");
        doc.replace_section(NEXT_ACTIONS_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format_action_done_log_entry(slug, &removed_full_text);

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        // If the completed bullet wikilinks an action note, archive the
        // note in the same transaction — its move to `_done/<year>/`
        // and the bullet removal are then atomic. A plain bullet skips
        // this and behaves exactly as before. Still one daily-log line,
        // not two.
        if let Some(action_slug) = parse_attached_action_slug(&removed_full_text) {
            self.stage_action_archival(at, action_slug, &mut tx)?;
        }
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        let touched = tx.commit()?;

        Ok(WriteOutcome::written(path, touched))
    }

    /// Promote an open bullet to a manifest action note (design §5.11).
    /// Finds the bullet via case-insensitive substring (same matching
    /// as `complete_action`), spins a new action note inheriting the
    /// bullet's title and energy, and rewrites the bullet to wikilink
    /// the note — all atomic, in a single transaction.
    ///
    /// Errors:
    /// - `ActionAlreadyPromoted` — the matched bullet already
    ///   wikilinks an action note.
    /// - `BulletMissingEnergy` — the bullet has no
    ///   `(deep|medium|light)` suffix to inherit; surfaced rather than
    ///   guessed so an authoring bug is visible.
    /// - `ActionNotFound` / `AmbiguousAction` — same disambiguation as
    ///   `complete_action`.
    /// - parked project → `ProjectNotActive`; missing project →
    ///   `Store(NotFound)`; slug collision on the new note →
    ///   `Store(AlreadyExists)`.
    pub fn promote_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
    ) -> Result<VaultPath, DomainError> {
        self.promote_action_with_vars(at, slug, query, &HashMap::new())
    }

    /// As [`promote_action`](Self::promote_action), with caller-supplied
    /// prompted-variable values (`[variables.prompt]`, #238) for the action
    /// note the promotion scaffolds. The CLI gathers these up front; other
    /// callers use the no-vars wrapper above.
    pub fn promote_action_with_vars(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
        prompted: &HashMap<String, String>,
    ) -> Result<VaultPath, DomainError> {
        // One transaction opened before the project read, so the whole
        // read-modify-write (find the bullet, spin the note, rewrite the
        // bullet) serialises under one write lock (#196).
        let mut tx = self.transaction()?;
        let (project_path, mut doc) = self.resolve_active_project(slug)?;

        let section = doc.section(NEXT_ACTIONS_SECTION)?;
        let lines: Vec<&str> = section.split('\n').collect();
        let bullet_idx = resolve_open_action(&lines, slug, query)?;
        let bullet_text = parse_open_action_text(lines[bullet_idx])
            .expect("matched line was previously parseable")
            .to_owned();

        if parse_attached_action_slug(&bullet_text).is_some() {
            return Err(DomainError::ActionAlreadyPromoted {
                slug: slug.to_owned(),
                line: bullet_text,
            });
        }

        let energy =
            parse_bullet_energy(&bullet_text).ok_or_else(|| DomainError::BulletMissingEnergy {
                slug: slug.to_owned(),
                line: bullet_text.clone(),
            })?;
        let title = strip_energy_suffix(&bullet_text).trim().to_owned();

        // Spin the note onto the existing transaction so note write +
        // bullet rewrite + daily log commit together, all under one lock.
        let note_path =
            self.create_action_note(&mut tx, at, slug, &title, energy, None, None, prompted)?;
        let action_slug = note_path
            .as_path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let new_bullet = format!(
            "- [ ] [[{}/{action_slug}]] ({})",
            cdno_core::paths::ACTIONS,
            energy.as_str()
        );
        let mut new_lines: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
        new_lines[bullet_idx] = new_bullet;
        let new_section = new_lines.join("\n");
        doc.replace_section(NEXT_ACTIONS_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let project_entry =
            build_index_entry_for(&project_path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format!(
            "action promoted on [[{slug}]] — \"{title}\" -> [[{}/{action_slug}]]",
            cdno_core::paths::ACTIONS,
        );

        tx.write_file(project_path, new_content);
        tx.upsert_note(project_entry);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(note_path)
    }

    /// List the open action bullets of an active project, resolving
    /// any wikilink bullets to their attached note's current status.
    ///
    /// Read-only: no writes, no daily log. A wikilink to a note that's
    /// since been archived (moved to `_done/<year>/`) or genuinely
    /// missing surfaces as `attached: None`; the bullet text itself
    /// still carries the wikilink so the caller can flag drift.
    /// Errors with `ProjectNotActive` on parked projects (listing
    /// queued work on a parked project is rarely useful — activate it
    /// first) and propagates any malformed-frontmatter parse errors
    /// from the attached notes.
    pub fn list_actions(&self, slug: &str) -> Result<Vec<ActionListEntry>, DomainError> {
        let (_path, doc) = self.resolve_active_project(slug)?;
        let Ok(section) = doc.section(NEXT_ACTIONS_SECTION) else {
            return Ok(Vec::new());
        };

        let mut out = Vec::new();
        for line in section.split('\n') {
            let Some(bullet_text) = parse_open_action_text(line) else {
                continue;
            };
            let text = bullet_text.to_owned();
            let energy = parse_bullet_energy(&text);
            let attached = parse_attached_action_slug(&text)
                .map(|s| s.to_owned())
                .map(
                    |action_slug| -> Result<Option<AttachedAction>, DomainError> {
                        let note_path = VaultPath::new(format!(
                            "{}/{action_slug}.md",
                            cdno_core::paths::ACTIONS
                        ))?;
                        if !self.store.exists(&note_path)? {
                            return Ok(None);
                        }
                        let raw = self.store.read_file(&note_path)?;
                        let (fm, _body) = Frontmatter::parse(&raw)?;
                        let af = ActionFrontmatter::try_from(fm)?;
                        Ok(Some(AttachedAction {
                            slug: action_slug,
                            status: af.status,
                        }))
                    },
                )
                .transpose()?
                .flatten();
            out.push(ActionListEntry {
                text,
                energy,
                attached,
            });
        }
        Ok(out)
    }
}

/// If `text` is a wikilink to an action note — `[[actions/<slug>]]`,
/// optionally followed by a `(<energy>)` suffix — return the slug.
/// Plain action bullets, links carrying a `|label`, and anything that
/// isn't exactly an `actions/` wikilink return `None`, so completion
/// falls through to the unchanged plain-bullet path.
fn parse_attached_action_slug(text: &str) -> Option<&str> {
    let inner = strip_energy_suffix(text.trim())
        .trim()
        .strip_prefix("[[")?
        .strip_suffix("]]")?;
    let slug = inner.strip_prefix("actions/")?;
    if slug.is_empty() || slug.contains(['[', ']', '|']) {
        None
    } else {
        Some(slug)
    }
}

/// Build the daily-log entry recording an action addition.
fn format_action_added_log_entry(slug: &str, action: &str, energy: EnergyLevel) -> String {
    format!(
        "action added to [[{slug}]] — {action} ({})",
        energy.as_str()
    )
}

/// The marker opening a daily-log line that records an action being
/// started. Shared with the reader ([`Vault::current_focus`]) so the two
/// cannot drift: the log IS the record of what you are on, and a parser
/// keyed on a different string would simply never find anything.
pub(in crate::vault) const LOG_STARTED_PREFIX: &str = "started ";
/// The marker for the line recording that action being finished.
pub(in crate::vault) const LOG_ACTION_DONE_PREFIX: &str = "action done on ";

/// Build the daily-log entry recording an action being started.
fn format_action_started_log_entry(slug: &str, action_text: &str) -> String {
    format!("{LOG_STARTED_PREFIX}[[{slug}]] \u{2014} {action_text}")
}

/// Build the daily-log entry recording an action completion.
/// `action_text` is the raw text from the project line, including
/// any `(<energy>)` suffix, so the historical record preserves what
/// energy bucket the action sat in.
fn format_action_done_log_entry(slug: &str, action_text: &str) -> String {
    format!("{LOG_ACTION_DONE_PREFIX}[[{slug}]] — {action_text}")
}

/// If `line` is an open action bullet (`- [ ] <text>`), return the
/// `<text>` verbatim — including any trailing `(<energy>)` suffix.
/// Closed bullets (`- [x]`), blanks, and non-bullet content return
/// `None`. Substring matching strips the suffix separately via
/// [`strip_energy_suffix`]; the verbatim form is what gets logged
/// on completion so the daily log preserves the energy tag.
fn parse_open_action_text(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("- [ ] ").map(str::trim)
}

/// Trim a trailing `(deep)`, `(medium)`, or `(light)` suffix —
/// matching is case-sensitive because `add_action` always emits
/// Find the one open action bullet `query` names, among `lines`.
///
/// Two rules, in order.
///
/// **An exact match on the whole bullet wins outright.** Every caller
/// already holds the full text — `list_actions` returns it verbatim, the
/// daily log records it verbatim, and the ambiguity picker hands back the
/// candidate it was given — so the common path should be precise, not
/// approximate. Without this, two bullets differing only by energy strip
/// to the same phrase and the picker's own answer re-ambiguates, leaving
/// the user unable to resolve their own choice.
///
/// **Otherwise, substring, with the energy suffix stripped from both
/// sides.** Every action the tool creates carries a suffix, so a query
/// echoing text the caller was shown arrives suffixed and would never
/// match a candidate whose suffix had been removed. A bare phrase typed by
/// hand is unaffected.
///
/// Shared by completion and promotion so the two cannot drift: they are
/// documented as behaving alike, and for a while they did not.
fn resolve_open_action(lines: &[&str], slug: &str, query: &str) -> Result<usize, DomainError> {
    let trimmed = query.trim();
    let exact = trimmed.to_lowercase();
    let exact_matches: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| parse_open_action_text(line).is_some_and(|t| t.to_lowercase() == exact))
        .map(|(i, _)| i)
        .collect();
    if exact_matches.len() == 1 {
        return Ok(exact_matches[0]);
    }

    let needle = strip_energy_suffix(trimmed).to_lowercase();
    let matches: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            parse_open_action_text(line)
                .is_some_and(|t| strip_energy_suffix(t).to_lowercase().contains(&needle))
        })
        .map(|(i, _)| i)
        .collect();

    match matches.len() {
        0 => Err(DomainError::ActionNotFound {
            slug: slug.to_owned(),
            query: query.to_owned(),
        }),
        1 => Ok(matches[0]),
        _ => Err(DomainError::AmbiguousAction {
            slug: slug.to_owned(),
            query: query.to_owned(),
            candidates: matches
                .iter()
                .map(|&i| parse_open_action_text(lines[i]).unwrap_or("").to_owned())
                .collect(),
        }),
    }
}

/// lowercase.
fn strip_energy_suffix(text: &str) -> &str {
    for suffix in [" (deep)", " (medium)", " (light)"] {
        if let Some(stripped) = text.strip_suffix(suffix) {
            return stripped;
        }
    }
    text
}

/// Recover the [`EnergyLevel`] from a bullet's trailing
/// `(deep|medium|light)` suffix; `None` for any other shape. Callers
/// decide whether the absence is an error (promote needs it) or
/// silently OK (completion just logs the raw text).
fn parse_bullet_energy(text: &str) -> Option<EnergyLevel> {
    if text.ends_with(" (deep)") {
        Some(EnergyLevel::Deep)
    } else if text.ends_with(" (medium)") {
        Some(EnergyLevel::Medium)
    } else if text.ends_with(" (light)") {
        Some(EnergyLevel::Light)
    } else {
        None
    }
}
