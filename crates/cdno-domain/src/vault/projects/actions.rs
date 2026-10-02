//! `add_action` / `complete_action`: mutate the `## Next Actions`
//! checklist of an active project, with daily-note logging on both
//! the addition (planning trace) and the completion.

use std::collections::HashMap;

use chrono::{NaiveDate, NaiveDateTime, Timelike};

use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{ActionFrontmatter, ActionStatus, EnergyLevel};
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::WriteOutcome;
use super::super::closure::Closure;
use super::super::context::{CurrentFocus, LastPause, resumed_focus};
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
pub struct AttachedAction {
    pub slug: String,
    pub status: ActionStatus,
}

/// What [`Vault::pause_action`] did: the focus that was paused and the daily
/// note that now records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PauseOutcome {
    pub paused: CurrentFocus,
    pub path: VaultPath,
}

/// What [`Vault::switch_action`] and [`Vault::switch_unplanned_action`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchOutcome {
    /// The focus that was paused, or `None` when nothing was open and the
    /// switch was a plain start.
    pub paused: Option<CurrentFocus>,
    /// The new focus.
    pub started: CurrentFocus,
    /// The note the operation is about (see each verb).
    pub primary: VaultPath,
    /// Every path the commit wrote.
    pub paths: Vec<VaultPath>,
}

/// What [`Vault::resume_action`] did: the focus it reopened, where that
/// focus came from, and the daily note that now records the resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeOutcome {
    /// The focus as the reader now sees it: stamped at the resume, with an
    /// `origin` only when a carried focus was continued.
    pub resumed: CurrentFocus,
    /// What was resumed.
    pub from: ResumedFrom,
    /// The daily note the `resumed` line was written to.
    pub path: VaultPath,
}

/// Where a resumed focus came from (RFC 0005 §5.3, `resumed_from`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumedFrom {
    /// Whether a carried focus was continued or a pause reopened.
    pub kind: ResumedKind,
    /// The date of the carried focus's open marker, or of the pause.
    pub date: chrono::NaiveDate,
    /// The pause's `next:` hint, as written. Always `None` for a carried focus.
    pub next: Option<String>,
    /// The pause's `reason:`, as written. Always `None` for a carried focus.
    pub reason: Option<String>,
}

/// Whether [`Vault::resume_action`] continued a focus carried over from an
/// earlier day or reopened a paused one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumedKind {
    Carried,
    Paused,
}

/// Everything `start_unplanned_action` writes, built but not yet staged.
struct UnplannedStart {
    path: VaultPath,
    new_content: String,
    entry_meta: cdno_core::index::NoteEntry,
    started_text: String,
    added_entry: String,
    started_entry: String,
}

impl UnplannedStart {
    /// Stage the map write and its index upsert, handing back the
    /// `(added, started)` log entries for the caller's daily-log write.
    fn stage(self, tx: &mut cdno_core::transaction::VaultTransaction) -> (String, String) {
        tx.write_file(self.path, self.new_content);
        tx.upsert_note(self.entry_meta);
        (self.added_entry, self.started_entry)
    }
}

/// The log lines of a switch, in order: `paused` [, `added`...], `started`.
fn switch_entries(
    open: Option<&CurrentFocus>,
    next: Option<&str>,
    reason: Option<&str>,
    added: &[String],
    started: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(f) = open {
        out.push(format_action_paused_log_entry(
            &f.project, &f.action, next, reason,
        ));
    }
    out.extend(added.iter().cloned());
    out.push(started.to_owned());
    out
}

/// The one place a [`DomainError::FocusOpen`] is built: `same_action` is a plain
/// equality against the resolved project and bullet text, and `carried` says
/// the focus's open marker sits in an earlier day's note than `at`'s.
fn focus_open_error(
    open: CurrentFocus,
    at: NaiveDateTime,
    slug: &str,
    target: &str,
) -> DomainError {
    DomainError::FocusOpen {
        same_action: open.project == slug && open.action == target,
        carried: open.date != at.date(),
        focus: open,
    }
}

/// `at` as the log line written at `at` records it: truncated to `HH:MM`.
fn log_stamp(at: NaiveDateTime) -> NaiveDateTime {
    at.date().and_time(
        chrono::NaiveTime::from_hms_opt(at.hour(), at.minute(), 0)
            .expect("a valid time truncated to the minute"),
    )
}

/// The focus a start at `at` opens; the stamp is the log line's `HH:MM`.
fn started_focus(at: NaiveDateTime, slug: &str, action: &str) -> CurrentFocus {
    CurrentFocus {
        project: slug.to_owned(),
        action: action.to_owned(),
        started: log_stamp(at).time(),
        date: at.date(),
        origin: None,
    }
}

impl Vault {
    /// Refuse a start while something is in focus. `slug`/`target` are the
    /// resolved project and bullet text, so `same_action` is a plain equality.
    fn refuse_if_focus_open(
        &self,
        at: NaiveDateTime,
        slug: &str,
        target: &str,
    ) -> Result<(), DomainError> {
        match self.current_focus(at.date())? {
            None => Ok(()),
            Some(open) => Err(focus_open_error(open, at, slug, target)),
        }
    }

    /// Resolve `query` against the project's `## Next Actions` exactly as the
    /// start verbs do and return the matched bullet's text. Shared by
    /// `start_action` and `switch_action` so the two cannot disagree (#568).
    fn resolve_start_target(&self, slug: &str, query: &str) -> Result<String, DomainError> {
        let (_path, doc) = self.resolve_active_project(slug)?;
        let section = doc.section(NEXT_ACTIONS_SECTION)?;
        let lines: Vec<&str> = section.split('\n').collect();
        let idx = resolve_open_action(&lines, slug, query)?;
        Ok(parse_open_action_text(lines[idx])
            .expect("matched line was previously parseable")
            .to_owned())
    }

    /// Build, without staging, everything `start_unplanned_action` writes: the
    /// project map with the new bullet appended and the two log entries. Shared
    /// with `switch_unplanned_action`.
    fn plan_unplanned_start(
        &self,
        slug: &str,
        action_text: &str,
        energy: EnergyLevel,
    ) -> Result<UnplannedStart, DomainError> {
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let bullet = format!("- [ ] {action_text} ({})", energy.as_str());
        // Read the started text back out of the bullet we are about to build,
        // exactly as `start_action` reads it out of one already on the
        // map — so the energy suffix and spacing match what the close
        // verbs will log, without this path knowing the format itself.
        let started_text = parse_open_action_text(&bullet)
            .expect("bullet was just formatted as `- [ ] …`")
            .to_owned();

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
        Ok(UnplannedStart {
            added_entry: format_action_added_log_entry(slug, action_text, energy),
            started_entry: format_action_started_log_entry(slug, &started_text),
            started_text,
            path,
            new_content,
            entry_meta,
        })
    }

    /// Switch focus to `query` in one commit: pause what is open (if anything),
    /// then start the target.
    ///
    /// The target is resolved first and the paused entry is built from the
    /// focus read in the same transaction; every line goes through one
    /// `stage_daily_logs` call (`paused`, `started`), so a failed resolution
    /// leaves no pause line behind. It composes the staging helpers rather than
    /// calling [`Vault::pause_action`] and [`Vault::start_action`], because the
    /// write lock is not re-entrant.
    ///
    /// With nothing open it is a plain start: `next` and `reason` are ignored
    /// and `paused` is `None`. Switching to the bullet already in focus is
    /// [`DomainError::FocusOpen`] with `same_action` set, and writes nothing.
    /// `primary` is the daily note, the only file written.
    ///
    /// Errors: blank `query` → `EmptyField`; parked → `ProjectNotActive`;
    /// missing project → `Store(NotFound)`; missing section → `Manipulation`;
    /// no match → [`DomainError::ActionNotFound`]; several matches →
    /// [`DomainError::AmbiguousAction`] with the candidates; the target is
    /// already the focus → [`DomainError::FocusOpen`] with `same_action: true`.
    /// Resolution errors win over `FocusOpen`, and every error leaves the vault
    /// untouched.
    pub fn switch_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
        next: Option<&str>,
        reason: Option<&str>,
    ) -> Result<SwitchOutcome, DomainError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(DomainError::EmptyField { field: "action" });
        }
        let mut tx = self.transaction()?;
        let action_text = self.resolve_start_target(slug, query)?;
        let open = self.focus_to_switch_from(at, slug, &action_text)?;

        let started_entry = format_action_started_log_entry(slug, &action_text);
        let entries = switch_entries(open.as_ref(), next, reason, &[], &started_entry);
        let refs: Vec<&str> = entries.iter().map(String::as_str).collect();
        let daily = self.stage_daily_logs(at, &refs, &mut tx)?;
        let paths = tx.commit()?;
        Ok(SwitchOutcome {
            paused: open,
            started: started_focus(at, slug, &action_text),
            primary: daily,
            paths,
        })
    }

    /// [`Vault::switch_action`] for work that was never planned: the bullet is
    /// added to the map as `start_unplanned_action` does, and the log gets
    /// `paused` [, `action added to`], `started` in one staged write.
    /// `primary` is the project map, as for `start_unplanned_action`.
    ///
    /// Errors: blank `title` → `EmptyField`; parked → `ProjectNotActive`;
    /// missing project → `Store(NotFound)`; a malformed map (for example two
    /// `## Next Actions` headings) → `Manipulation`; the new bullet's text equals
    /// the focused one → [`DomainError::FocusOpen`] with `same_action: true`.
    /// There is no not-found or ambiguity error: the action is being created.
    /// Map errors win over `FocusOpen`, and every error leaves the vault
    /// untouched.
    pub fn switch_unplanned_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        title: &str,
        energy: EnergyLevel,
        next: Option<&str>,
        reason: Option<&str>,
    ) -> Result<SwitchOutcome, DomainError> {
        let action_text = flatten_reason(title);
        if action_text.is_empty() {
            return Err(DomainError::EmptyField { field: "action" });
        }
        let mut tx = self.transaction()?;
        let plan = self.plan_unplanned_start(slug, &action_text, energy)?;
        let open = self.focus_to_switch_from(at, slug, &plan.started_text)?;

        let entries = switch_entries(
            open.as_ref(),
            next,
            reason,
            std::slice::from_ref(&plan.added_entry),
            &plan.started_entry,
        );
        let refs: Vec<&str> = entries.iter().map(String::as_str).collect();
        let started = started_focus(at, slug, &plan.started_text);
        let path = plan.path.clone();
        let _ = plan.stage(&mut tx);
        self.stage_daily_logs(at, &refs, &mut tx)?;
        let paths = tx.commit()?;
        Ok(SwitchOutcome {
            paused: open,
            started,
            primary: path,
            paths,
        })
    }

    /// The focus a switch to `slug`/`target` pauses, if any. Switching to the
    /// bullet that is already in focus is refused as [`DomainError::FocusOpen`].
    fn focus_to_switch_from(
        &self,
        at: NaiveDateTime,
        slug: &str,
        target: &str,
    ) -> Result<Option<CurrentFocus>, DomainError> {
        match self.current_focus(at.date())? {
            Some(open) if open.project == slug && open.action == target => {
                Err(focus_open_error(open, at, slug, target))
            }
            other => Ok(other),
        }
    }

    /// Pause the open focus: one `action paused on [[slug]] — <text>` line in
    /// the daily log, with optional `next:` and `reason:` continuations.
    ///
    /// The text logged is the focus's own, read back from the log, so this
    /// resolves nothing against the project map: a focus on a since-parked
    /// project can still be paused, and the bullet is left untouched. It
    /// therefore never returns `ProjectNotActive` or `ActionNotFound`.
    ///
    /// Errors: nothing open within the focus window ([`Vault::current_focus`])
    /// → [`DomainError::NoFocus`].
    pub fn pause_action(
        &self,
        at: NaiveDateTime,
        next: Option<&str>,
        reason: Option<&str>,
    ) -> Result<PauseOutcome, DomainError> {
        let mut tx = self.transaction()?;
        let paused = self.current_focus(at.date())?.ok_or(DomainError::NoFocus)?;
        let entry = format_action_paused_log_entry(&paused.project, &paused.action, next, reason);
        let path = self.stage_daily_log(at, &entry, &mut tx)?;
        tx.commit()?;
        Ok(PauseOutcome { paused, path })
    }

    /// The pause a resume would reopen: `project`'s newest open pause, or the
    /// newest across projects when `None` (one [`Vault::last_pauses`] scan).
    fn resumable_pause(
        &self,
        today: NaiveDate,
        project: Option<&str>,
    ) -> Result<Option<LastPause>, DomainError> {
        let pauses = self.last_pauses(today)?;
        Ok(match project {
            Some(slug) => pauses.by_project.get(slug).cloned(),
            None => pauses.latest,
        })
    }

    /// Resume work: one `resumed [[slug]] — <text>` line in the daily log,
    /// which re-anchors the focus at `at` (RFC 0005 §5.3, D9).
    ///
    /// The text comes from the log, never from the map, so this resolves
    /// nothing against the project map, like [`Vault::pause_action`]. What is
    /// resumed:
    ///
    /// - The carried focus — [`Vault::current_focus`] dated before `at`'s
    ///   day — if there is one and it is on `project` (any project when
    ///   `project` is `None`);
    /// - else the most recent pause within `[focus] paused_lookback_days`
    ///   (that project's, when one is given) that no later line of the same
    ///   action consumed: a `started` or `resumed` of it, its `done` or
    ///   `dropped`, or a promotion of its bullet.
    ///
    /// Resuming the carried focus continues it, so the result keeps its
    /// `origin`; resuming a pause is a fresh open with no `origin`, and
    /// `from` carries the pause's date and its `next:` / `reason:` lines.
    /// `resumed` is the focus [`Vault::current_focus`] reads back after the
    /// commit, built by the same function the reader's fold uses.
    ///
    /// Errors, every one leaving the vault untouched: nothing resumable →
    /// [`DomainError::NoFocus`]; a pause chosen while a different focus is in
    /// the slot, open today or carried → [`DomainError::FocusOpen`] (resuming
    /// over a carried focus would displace it with no pause line, so it is
    /// refused like one open today); no pause to resume and a focus already
    /// anchored today on `project` (any project when `project` is `None`) →
    /// [`DomainError::FocusOpen`] with `same_action` (already focused).
    pub fn resume_action(
        &self,
        at: NaiveDateTime,
        project: Option<&str>,
    ) -> Result<ResumeOutcome, DomainError> {
        let mut tx = self.transaction()?;
        let today = at.date();
        let open = self.current_focus(today)?;
        let project = project.map(str::trim);

        let ((slug, action), from) = match (&open, project) {
            // A carried focus on the asked-for project (any, for the bare
            // verb): continue it.
            (Some(carried), proj)
                if carried.date != today && proj.is_none_or(|p| p == carried.project) =>
            {
                (
                    (carried.project.clone(), carried.action.clone()),
                    ResumedFrom {
                        kind: ResumedKind::Carried,
                        date: carried.date,
                        next: None,
                        reason: None,
                    },
                )
            }
            _ => match self.resumable_pause(today, project)? {
                Some(pause) => {
                    // The slot is taken — today or carried — so reopening the
                    // pause would displace it with no pause line.
                    if let Some(open) = open {
                        return Err(focus_open_error(open, at, &pause.project, &pause.action));
                    }
                    let from = ResumedFrom {
                        kind: ResumedKind::Paused,
                        date: pause.at.date(),
                        next: pause.next,
                        reason: pause.reason,
                    };
                    ((pause.project, pause.action), from)
                }
                None => {
                    return Err(match (open, project) {
                        // Nothing to resume, and today's focus in the slot is
                        // the asked-for project's (any, for the bare verb):
                        // it is already focused.
                        (Some(open), proj) if proj.is_none_or(|p| p == open.project) => {
                            let (slug, action) = (open.project.clone(), open.action.clone());
                            focus_open_error(open, at, &slug, &action)
                        }
                        _ => DomainError::NoFocus,
                    });
                }
            },
        };

        let entry = format_resumed_log_entry(&slug, &action);
        let path = self.stage_daily_log(at, &entry, &mut tx)?;
        tx.commit()?;
        // Exactly what the fold reads back from the line just written: the
        // slot it displaces is `open`, which is the carried focus itself or,
        // for a pause, empty.
        let resumed = resumed_focus(open.as_ref(), log_stamp(at), &slug, &action);
        Ok(ResumeOutcome {
            resumed,
            from,
            path,
        })
    }

    /// Record that work on an action is starting: one line in today's
    /// daily log, `started [[<slug>]] — <action>`.
    ///
    /// This is the single home of the "started" log format — CLI,
    /// MCP, and desktop-Start-button surfaces are expected to call
    /// this rather than compose their own line, so the trace stays
    /// greppable. The project is resolved first (active projects only)
    /// so the logged wikilink can't dangle.
    ///
    /// `action` is a **query against `## Next Actions`**, matched by
    /// [`resolve_open_action`] exactly as `complete_action` and
    /// `drop_action` match theirs, and the *resolved bullet text* is
    /// what gets logged — not the string passed in.
    ///
    /// That sharing is the point (#568). [`Vault::current_focus`] pairs
    /// this entry with the closing one by exact text equality, and the
    /// close verbs log resolved text; a start logged verbatim is
    /// closable only while the caller happens to pass exactly what they
    /// will later write. The desktop app does — it passes
    /// `ActionListEntry::text`, which is the bullet verbatim — but that
    /// was caller discipline rather than a guarantee, and a second
    /// caller had no way to know the rule.
    ///
    /// **This used to accept free text**, on the reasoning that
    /// "starting unplanned work is equally valid". It was not: an
    /// unplanned start names no bullet, so no completion can ever log
    /// matching text, and the focus stays open for ever. Demonstrated
    /// before the change — `start_action(.., "Buy milk")` on a project
    /// with no such bullet left `current_focus` reporting `Buy milk`
    /// permanently, with `complete_action` refusing it as not found.
    /// Refusing at the start turns a silent, unfixable state into an
    /// error where the mistake is.
    ///
    /// Returns the daily-note path touched. Errors mirror the other
    /// action ops: parked → `ProjectNotActive`, missing →
    /// `Store(NotFound)`, whitespace-only action → `EmptyField`,
    /// missing section → `Manipulation`, no match →
    /// [`DomainError::ActionNotFound`], several matches →
    /// [`DomainError::AmbiguousAction`] carrying the candidates, and
    /// [`DomainError::FocusOpen`] when an action is already in focus (checked
    /// last, so the errors above win).
    pub fn start_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        action: &str,
    ) -> Result<VaultPath, DomainError> {
        let query = action.trim();
        if query.is_empty() {
            return Err(DomainError::EmptyField { field: "action" });
        }
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let action_text = self.resolve_start_target(slug, query)?;
        let action_text = action_text.as_str();
        // After resolution, so a typo is still a typo (RFC 0005 §5.1).
        self.refuse_if_focus_open(at, slug, action_text)?;

        let log_entry = format_action_started_log_entry(slug, action_text);
        let daily_path = self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;
        Ok(daily_path)
    }

    /// Start work that was never planned: append `action` to the
    /// project's `## Next Actions` and log it as started, in one commit.
    ///
    /// This is the honest form of the gesture [`Vault::start_action`]
    /// used to appear to support. Its old doc claimed "starting
    /// unplanned work is equally valid", but free-text starts never
    /// worked (#568): [`Vault::current_focus`] pairs a start with its
    /// close by exact text equality, and the close verbs only ever log
    /// text they resolved *from a bullet*, so work with no bullet could
    /// be started and then never closed — the focus stayed pinned to it
    /// for ever.
    ///
    /// So rather than logging a start that nothing can close, this
    /// gives the work a bullet first and starts *that*. The action
    /// becomes ordinary planned work the moment it begins, closable by
    /// `complete_action` and `drop_action` like any other. The started
    /// line is derived from the bullet just written, by the same
    /// [`parse_open_action_text`] the close verbs resolve through, so
    /// the texts agree by construction rather than by both sides
    /// formatting the string the same way.
    ///
    /// [`Vault::promote_action`] also resolves through [`resolve_open_action`]
    /// but **rewrites** the bullet it matched. The reader follows it: the
    /// promotion line it logs renames the open start, so the later close
    /// still pairs (`a_promotion_between_start_and_close_moves_the_focus_to_the_note`).
    ///
    /// Keep this separate from `start_action` rather than making it a
    /// fallback when the query matches nothing: a fallback would turn
    /// every typo into a *new* action silently — the exact class of
    /// failure #568 removed. Creating work is a different intent from
    /// starting known work, so the caller states which it means.
    ///
    /// **Two log lines, deliberately**: `action added to …` then
    /// `started …`. Adding the bullet mutates `## Next Actions`, and the
    /// vault's history rule is that a mutable-section change emits its
    /// own log entry — so the bullet's origin stays greppable instead of
    /// appearing on the map from nowhere.
    ///
    /// Returns a [`WriteOutcome`] like the close verbs: `primary` is the
    /// project map, `paths` every file the commit wrote (the map and the
    /// daily note), which is what the desktop journals for the watcher
    /// (#315). `add_action` writes the same two files and gets away with
    /// returning a bare path because its caller rebuilds the daily path
    /// from the same clock — so this is the safer shape, not the only
    /// workable one: the touched set stays right here if a later change
    /// makes this verb write a third file, where a caller-side rebuild
    /// would silently keep journalling two.
    ///
    /// Errors: parked → `ProjectNotActive`, missing → `Store(NotFound)`,
    /// whitespace-only action → `EmptyField`. The first two mirror
    /// [`Vault::add_action`]; the blank check does not — `add_action` has
    /// none and will write `- [ ]  (deep)` — this mirrors `start_action`,
    /// since starting nameless work is the failure #568 is about. There
    /// is no not-found or ambiguity error here: the action is being
    /// created, so there is nothing to match against. An action already in
    /// focus → [`DomainError::FocusOpen`], checked before anything is written.
    ///
    /// Whitespace inside `action` is flattened, as [`flatten_reason`]
    /// does for a drop reason and for the same reason: an interior
    /// newline would split the bullet across two lines of
    /// `## Next Actions` — leaving an orphan non-bullet line behind when
    /// the action is later closed — and split each log entry into a
    /// second physical line no reader parses. `add_action` has the same
    /// hole and is left alone here rather than widening this change.
    pub fn start_unplanned_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        action: &str,
        energy: EnergyLevel,
    ) -> Result<WriteOutcome, DomainError> {
        // Flattened, not merely trimmed: an interior newline would split
        // the bullet and both log entries across physical lines.
        let action_text = flatten_reason(action);
        let action_text = action_text.as_str();
        if action_text.is_empty() {
            return Err(DomainError::EmptyField { field: "action" });
        }
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let plan = self.plan_unplanned_start(slug, action_text, energy)?;
        // Before anything is staged: a refused start creates nothing.
        self.refuse_if_focus_open(at, slug, &plan.started_text)?;

        let path = plan.path.clone();
        let (added_entry, started_entry) = plan.stage(&mut tx);
        // One staged write for both lines — see `stage_daily_logs`;
        // staging them separately would drop the first.
        self.stage_daily_logs(at, &[&added_entry, &started_entry], &mut tx)?;
        let touched = tx.commit()?;

        Ok(WriteOutcome::written(path, touched))
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

        let new_section = remove_action_line(section, removed_idx);
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
            self.stage_action_archival(at, action_slug, Closure::Completed, &mut tx)?;
        }
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        let touched = tx.commit()?;

        Ok(WriteOutcome::written(path, touched))
    }

    /// Remove an open action from an active project **without claiming
    /// it was done** (#559), logging the drop to today's daily note.
    ///
    /// `complete_action` was the only verb that removed a bullet, and it
    /// writes `action done on [[slug]] — <text>`. So an action that was
    /// superseded, abandoned or reprioritised could only be cleared by
    /// recording work that never happened — into the daily log, which is
    /// the record the weekly review, the monthly scan and every later
    /// verdict read back from. `docs-site/src/concepts/rlm.md` promises
    /// "permission to park or drop"; this is actions getting it.
    ///
    /// Matching, ambiguity and the attached-note handling are identical
    /// to [`complete_action`](Self::complete_action) — deliberately, so
    /// there is nothing new to learn to use it. The differences are the
    /// log prefix, the optional `reason`, and that the archived note is
    /// stamped `status: dropped` with no completion date.
    ///
    /// `reason` is optional but wanted: "superseded by X" and "no longer
    /// wanted" are different facts, and the distinction is exactly what
    /// a later reader needs. Whitespace in it is flattened so the entry
    /// stays one log line.
    ///
    /// Returns a [`WriteOutcome`] with the same shape as
    /// `complete_action`: `primary` is the project map, `paths` every
    /// file the commit wrote.
    pub fn drop_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
        reason: Option<&str>,
    ) -> Result<WriteOutcome, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let section = doc.section(NEXT_ACTIONS_SECTION)?;
        let lines: Vec<&str> = section.split('\n').collect();
        let removed_idx = resolve_open_action(&lines, slug, query)?;
        let removed_full_text = parse_open_action_text(lines[removed_idx])
            .expect("matched line was previously parseable")
            .to_owned();

        let new_section = remove_action_line(section, removed_idx);
        doc.replace_section(NEXT_ACTIONS_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format_action_dropped_log_entry(slug, &removed_full_text, reason);

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        if let Some(action_slug) = parse_attached_action_slug(&removed_full_text) {
            self.stage_action_archival(at, action_slug, Closure::Dropped, &mut tx)?;
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
            "{LOG_ACTION_PROMOTED_PREFIX}[[{slug}]] — \"{title}\" -> [[{}/{action_slug}]]",
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
pub(in crate::vault) fn parse_attached_action_slug(text: &str) -> Option<&str> {
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
/// The marker for the line recording an action being **dropped** rather
/// than performed (#559). A separate prefix, not a variation on the
/// done one, because the whole point is that a later reader — a weekly
/// review, a monthly scan, a person — can tell the two apart. Shared
/// with [`Vault::current_focus`], which must clear an open start on a
/// drop as well as on a completion, or an abandoned action stays "what
/// you are on" for ever.
pub(in crate::vault) const LOG_ACTION_DROPPED_PREFIX: &str = "action dropped on ";
/// The marker for the line [`Vault::promote_action`] writes when it
/// rewrites an inline bullet into an attached action note. Shared with
/// [`Vault::current_focus`], which reads it as a **rename** of the open
/// start, so the focus follows the bullet to its note.
pub(in crate::vault) const LOG_ACTION_PROMOTED_PREFIX: &str = "action promoted on ";

/// The marker for the line recording that work on an action is resumed
/// (RFC 0005 §5.3). An **open** marker: [`Vault::current_focus`] reads it as
/// close-plus-reopen at its own stamp, keeping the origin when it continues
/// the action already in the slot. Shared with lint so a malformed marker is
/// reported.
pub(in crate::vault) const LOG_RESUMED_PREFIX: &str = "resumed ";

/// Build the daily-log entry recording an action being resumed:
/// `resumed [[slug]] — <text>`, the text exactly as the focus or pause
/// logged it, so later closes pair with it by equality.
pub(in crate::vault) fn format_resumed_log_entry(slug: &str, action_text: &str) -> String {
    format!("{LOG_RESUMED_PREFIX}[[{slug}]] \u{2014} {action_text}")
}

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

/// Build the daily-log entry recording an action being dropped, with an
/// optional reason.
///
/// The reason is the part a later reader actually needs: "superseded by
/// the demo-planning action" and "no longer wanted" are different facts
/// about the project, and only one of them suggests looking for a
/// replacement.
///
/// It goes on an **indented continuation line**, the shape
/// `update_project_state` established for its `was:` / `now:` bodies,
/// rather than inline after the action text.
///
/// That is load-bearing, and worth stating precisely because the
/// obvious justification is wrong: `parse_log_lines` folds a
/// continuation into its entry joined with `"; "`, so an inline
/// `; reason: …` and a continuation line are byte-identical to any
/// reader that folds. What makes the shape matter is that
/// [`Vault::current_focus`] deliberately does **not** fold — it reads
/// entry heads, so the action text it compares is exactly this line,
/// with the reason on a line of its own where it cannot perturb the
/// match. Emit the reason inline and the head carries it, and every
/// drop-with-a-reason stops clearing its start.
///
/// Whitespace in the reason is flattened so one drop stays one entry.
pub(in crate::vault) fn format_action_dropped_log_entry(
    slug: &str,
    action_text: &str,
    reason: Option<&str>,
) -> String {
    let base = format!("{LOG_ACTION_DROPPED_PREFIX}[[{slug}]] \u{2014} {action_text}");
    match reason.map(flatten_reason).filter(|r| !r.is_empty()) {
        Some(reason) => format!("{base}\n  {LOG_REASON_KEY}{reason}"),
        None => base,
    }
}

/// Key introducing the reason on a dropped entry's continuation line.
/// Shared with `drop_milestone` and `drop_commitment` so every
/// abandonment verb writes the same shape. No reader parses it back today — `current_focus` matches
/// on entry heads and the reason lives below the head — but anything
/// that wants to read reasons later should key off this constant rather
/// than a fresh literal.
pub(in crate::vault) const LOG_REASON_KEY: &str = "reason: ";

/// The marker for the line recording that action being paused. Shared with
/// [`Vault::current_focus`], which must clear an open start on a pause as
/// well as on a completion or drop, or a paused action stays "what you are
/// on" for ever. Also shared with lint so a malformed marker is reported.
pub(in crate::vault) const LOG_ACTION_PAUSED_PREFIX: &str = "action paused on ";

/// Key introducing the next-action hint on a paused entry's continuation line.
pub(in crate::vault) const LOG_NEXT_KEY: &str = "next: ";

/// Build the daily-log entry recording an action being paused, with optional
/// continuations for where to pick up and why the pause happened.
///
/// The continuations follow the same rules as drops: they go on indented lines
/// so [`Vault::current_focus`] can read the head without folding and still match
/// it correctly. Both `next` and `reason` are optional and flattened independently.
pub fn format_action_paused_log_entry(
    slug: &str,
    action_text: &str,
    next: Option<&str>,
    reason: Option<&str>,
) -> String {
    let base = format!("{LOG_ACTION_PAUSED_PREFIX}[[{slug}]] \u{2014} {action_text}");
    let with_next = match next.map(flatten_reason).filter(|r| !r.is_empty()) {
        Some(next) => format!("{base}\n  {LOG_NEXT_KEY}{next}"),
        None => base,
    };
    match reason.map(flatten_reason).filter(|r| !r.is_empty()) {
        Some(reason) => format!("{with_next}\n  {LOG_REASON_KEY}{reason}"),
        None => with_next,
    }
}

/// Collapse every whitespace run — newlines included — to a single
/// space, so a multi-line reason cannot split one log entry into
/// several lines that no reader would parse as one. A thin re-export of
/// [`super::super::log::flatten_for_log`], the one flattener shared by
/// creation lines, action/milestone/commitment drop reasons, and inbox
/// discard captures.
pub(in crate::vault) fn flatten_reason(reason: &str) -> String {
    super::super::log::flatten_for_log(reason)
}

/// `section` without its line at `idx`, the index into
/// `section.split('\n')` that [`resolve_open_action`] returns. An action
/// bullet has no continuation lines of its own, so exactly one line goes.
///
/// Shared by `complete_action`, `drop_action` and the project cascade,
/// which removes several bullets inside one transaction and so cannot call
/// the verbs (each opens its own, and the lock is not re-entrant).
pub(in crate::vault) fn remove_action_line(section: &str, idx: usize) -> String {
    section
        .split('\n')
        .enumerate()
        .filter_map(|(i, line)| (i != idx).then_some(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// If `line` is an open action bullet (`- [ ] <text>`), return the
/// `<text>` verbatim — including any trailing `(<energy>)` suffix.
/// Closed bullets (`- [x]`), blanks, and non-bullet content return
/// `None`. Substring matching strips the suffix separately via
/// [`strip_energy_suffix`]; the verbatim form is what gets logged
/// on completion so the daily log preserves the energy tag.
pub(in crate::vault) fn parse_open_action_text(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("- [ ] ").map(str::trim)
}

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

/// Trim a trailing `(deep)`, `(medium)`, or `(light)` suffix —
/// matching is case-sensitive because `add_action` always emits
/// lowercase.
pub(in crate::vault) fn strip_energy_suffix(text: &str) -> &str {
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
pub(in crate::vault) fn parse_bullet_energy(text: &str) -> Option<EnergyLevel> {
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
