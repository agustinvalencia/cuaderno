//! `cdno action` subcommands: the user-facing surface for the action
//! layer. `add`, `promote`, and `complete` are thin clap-to-domain
//! shims; `list` reads `Vault::list_actions` and formats the bullets
//! with their attached-note status inline.
//!
//! Promptable fields are declared `Option<T>`. In a TTY (and unless
//! `--no-interactive` is set) a missing field is gathered via the
//! `prompt` module; in non-interactive sessions missing fields error
//! with a clear "missing --flag" message. The handler tracks whether
//! anything was prompted and renders a preview-and-confirm step in
//! that case, matching the design's "confirm-on-prompt only" rule.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use clap::Subcommand;
use clap_complete::engine::ArgValueCompleter;

use cdno_domain::frontmatter::{ActionStatus, EnergyLevel};
use cdno_domain::{ActionListEntry, AttachedAction, Vault};

use crate::bootstrap;
use crate::completions;
use crate::output::style::{Palette, Role};
use crate::prompt;

#[derive(Debug, Subcommand)]
pub enum ActionCommands {
    /// Append a next action to a project. `--note` creates a manifest
    /// note alongside the bullet and wikilinks it.
    Add {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
        /// Action title.
        #[arg(long)]
        title: Option<String>,
        /// Energy bucket: deep, medium, or light.
        #[arg(long)]
        energy: Option<EnergyLevel>,
        /// Promote on creation: also write an action note and wikilink
        /// the bullet to it.
        #[arg(long)]
        note: bool,
        /// Value for a custom action-note template's prompted variable
        /// (`[variables.prompt]`), repeatable: `--var name=value`. Only
        /// applies with `--note` (a plain bullet isn't templated).
        #[arg(long = "var", value_parser = crate::prompt::parse_key_val)]
        var: Vec<(String, String)>,
    },

    /// Promote an existing plain bullet to a wikilinked manifest note.
    /// Substring-matches the bullet text; energy is inherited.
    Promote {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
        /// Substring matching the bullet to promote.
        #[arg(long)]
        query: Option<String>,
        /// Value for a custom action-note template's prompted variable
        /// (`[variables.prompt]`), repeatable: `--var name=value`.
        #[arg(long = "var", value_parser = crate::prompt::parse_key_val)]
        var: Vec<(String, String)>,
    },

    /// Start work on a next action: logs `started [[slug]] — <bullet>`
    /// to today's daily note, which is what `cdno now` reads back.
    ///
    /// The action must already be on the map. A start names a bullet so
    /// that the later completion logs matching text and the focus
    /// clears; a start that names nothing could never be closed (#568).
    /// `action promote` rewrites the bullet it matches; the focus
    /// follows it to the new note, so promoting between a start and its
    /// close is safe and the start time is kept.
    /// For work on no map yet, pass `--unplanned` with `--title` and
    /// `--energy` — that adds the bullet and starts it in one commit.
    Start {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
        /// Substring matching the open bullet to start.
        #[arg(long, conflicts_with_all = ["unplanned", "title", "energy"])]
        query: Option<String>,
        /// Start work that is on no map yet: adds the bullet, then
        /// starts it. Deliberately explicit rather than a fallback when
        /// `--query` matches nothing — a fallback would turn a typo into
        /// a new action silently.
        #[arg(long)]
        unplanned: bool,
        /// Title for the new bullet. Requires `--unplanned`.
        //
        // `requires` rather than a runtime check: without it, passing
        // `--title` and forgetting `--unplanned` is a dead end that
        // never names the missing flag -- non-interactively it asks for
        // `--query`, and interactively `fn start` takes the resolve
        // branch, discards the title, and offers the picker of
        // *existing* bullets, so a confirmed choice logs a start for
        // work the person did not name. Stated to clap rather than
        // checked at runtime, the way `templates eject` states its
        // exactly-one-of rule (`required_unless_present` +
        // `conflicts_with`): the parser then names the missing flag.
        // Kept as `//` so it stays out of `--help`, which no other flag
        // in this file uses to name internal Rust items.
        #[arg(long, requires = "unplanned")]
        title: Option<String>,
        /// Energy for the new bullet: `deep`, `medium` or `light`.
        /// Requires `--unplanned`.
        #[arg(long, requires = "unplanned")]
        energy: Option<EnergyLevel>,
    },

    /// Moves focus to another action: pauses the one in focus and starts
    /// this one, in one commit.
    ///
    /// The deliberate way past `start`'s refusal. Takes the same
    /// `--project`/`--query` (or `--unplanned` with `--title` and
    /// `--energy`) as `start`. In a terminal a missing `--next` is asked
    /// for once (Enter skips); `--reason` is never asked for. With
    /// nothing in focus it is a plain start and `--next` has nothing to
    /// attach to.
    Switch {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
        /// Substring matching the open bullet to switch to.
        #[arg(long, conflicts_with_all = ["unplanned", "title", "energy"])]
        query: Option<String>,
        /// Switch to work that is on no map yet: adds the bullet, then
        /// starts it.
        #[arg(long)]
        unplanned: bool,
        /// Title for the new bullet. Requires `--unplanned`.
        #[arg(long, requires = "unplanned")]
        title: Option<String>,
        /// Energy for the new bullet: `deep`, `medium` or `light`.
        /// Requires `--unplanned`.
        #[arg(long, requires = "unplanned")]
        energy: Option<EnergyLevel>,
        /// Where to pick the paused action up again.
        #[arg(long)]
        next: Option<String>,
        /// Why the focus moved. Never prompted for.
        #[arg(long)]
        reason: Option<String>,
    },

    /// Mark a next action as completed by case-insensitive substring
    /// match. A wikilinked bullet also archives its note to
    /// `actions/_done/<year>/`.
    Complete {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
        /// Substring matching the action to complete.
        #[arg(long)]
        query: Option<String>,
    },

    /// Drop a next action by case-insensitive substring match: closes it
    /// WITHOUT recording it as done. For work that was superseded,
    /// abandoned or reprioritised. A wikilinked bullet also archives its
    /// note to `actions/_done/<year>/`, stamped `status: dropped`.
    Drop {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
        /// Substring matching the action to drop.
        #[arg(long)]
        query: Option<String>,
        /// Why it was dropped ("superseded by X", "no longer wanted").
        /// Optional, but it is what a later reader needs: it is the
        /// difference between looking for a replacement and not.
        #[arg(long)]
        reason: Option<String>,
    },

    /// Pauses the current focus; takes no project or query.
    ///
    /// Logs `action paused on [[slug]] — <text>` to today's daily note
    /// and leaves the bullet on the map untouched. In a terminal, a
    /// missing `--next` is asked for once (Enter skips); `--reason` is
    /// never asked for.
    Pause {
        /// Where to pick up again: the re-entry hint `resume` and
        /// `cdno now` read back.
        #[arg(long)]
        next: Option<String>,
        /// Why the work was paused. Never prompted for.
        #[arg(long)]
        reason: Option<String>,
    },

    /// Resumes the carried or paused focus; takes no query or title.
    ///
    /// Re-anchors a focus carried over from an earlier day, or reopens
    /// the most recent pause, and prints the pause's `next:` hint when
    /// it has one. Refused while a different action is in focus.
    Resume {
        /// Resume the pause on this project instead of the latest one.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_any_project))]
        project: Option<String>,
    },

    /// List a project's open action bullets, with the attached-note
    /// status (active / blocked / completed / dropped) inline when present.
    List {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
    },
}

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: ActionCommands,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    run_with_vault_flag(root, None, at, command, no_interactive, json)
}

/// [`run`] for a caller that knows the person passed `--vault`: the
/// commands a refusal suggests then carry it, so they work as pasted.
pub fn run_with_vault_flag(
    root: &Path,
    vault_flag: Option<&Path>,
    at: NaiveDateTime,
    command: ActionCommands,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result. Scripted callers pass full args.
    let interactive = prompt::reports_interactively(no_interactive, json);

    match command {
        ActionCommands::Add {
            project,
            title,
            energy,
            note,
            var,
        } => add(
            &vault,
            at,
            project,
            title,
            energy,
            note,
            var,
            interactive,
            json,
        ),
        ActionCommands::Promote {
            project,
            query,
            var,
        } => promote(&vault, at, project, query, var, interactive, json),
        ActionCommands::Start {
            project,
            query,
            unplanned,
            title,
            energy,
        } => start(
            &vault,
            at,
            Target {
                project,
                query,
                unplanned,
                title,
                energy,
            },
            interactive,
            json,
            vault_flag,
        ),
        ActionCommands::Switch {
            project,
            query,
            unplanned,
            title,
            energy,
            next,
            reason,
        } => switch(
            &vault,
            at,
            Target {
                project,
                query,
                unplanned,
                title,
                energy,
            },
            next,
            reason,
            interactive,
            json,
            vault_flag,
        ),
        ActionCommands::Complete { project, query } => {
            complete(&vault, at, project, query, interactive, json)
        }
        ActionCommands::Drop {
            project,
            query,
            reason,
        } => drop_verb(&vault, at, project, query, reason, interactive, json),
        ActionCommands::Pause { next, reason } => {
            pause(&vault, at, next, reason, interactive, json)
        }
        ActionCommands::Resume { project } => resume(&vault, at, project, json),
        ActionCommands::List { project } => list(&vault, project, interactive, json),
    }
}

// ---------------------------------------------------------------------
// Per-verb handlers — gather missing fields, confirm-on-prompt, execute.
// ---------------------------------------------------------------------

#[allow(clippy::too_many_arguments)] // thin CLI gather→confirm→execute passthrough
fn add(
    vault: &Vault,
    at: NaiveDateTime,
    project: Option<String>,
    title: Option<String>,
    energy: Option<EnergyLevel>,
    note_flag: bool,
    var: Vec<(String, String)>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let project = prompt::gather_or_error(project, "project", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let title = prompt::gather_or_error(title, "title", interactive, &mut prompted, || {
        prompt::prompt_text("Title")
    })?;
    let energy = prompt::gather_or_error(energy, "energy", interactive, &mut prompted, || {
        prompt::prompt_energy()
    })?;
    // Only ask about --note when we're already in an interactive flow.
    // A user who provided every other flag and omitted --note clearly
    // wants the default (plain bullet).
    let note = if prompted {
        prompt::prompt_confirm("Promote on creation? (writes an action note)", note_flag)?
    } else {
        note_flag
    };

    // Prompted template variables apply only to the action *note*; a plain
    // bullet isn't templated, so `--var` is ignored without `--note`.
    let template_vars = if note {
        prompt::gather_template_vars(vault, "action", None, &var, interactive, &mut prompted)?
    } else {
        std::collections::HashMap::new()
    };

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to add to project '{project}':\n  title:  {title}\n  energy: {}\n  note:   {}",
            energy.as_str(),
            yesno(note),
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    if note {
        // `path` here is the new action NOTE (the with-note branch
        // scaffolds one); the plain branch below reports the project map.
        // Both are "the file written", just different files per branch.
        let path = vault
            .add_action_with_note_and_vars(at, &project, &title, energy, &template_vars)
            .context("adding action with note")?;
        crate::output::emit_write_result(
            json,
            &path.to_string(),
            &format!("Action added to projects/{project}.md with note {path}"),
        )?;
    } else {
        let path = vault
            .add_action(at, &project, &title, energy)
            .context("adding action")?;
        crate::output::emit_write_result(
            json,
            &path.to_string(),
            &format!("Action added to {path}"),
        )?;
    }
    Ok(())
}

/// What a `start` or `switch` was asked to act on, as typed: every
/// promptable field still optional.
pub struct Target {
    pub project: Option<String>,
    pub query: Option<String>,
    pub unplanned: bool,
    pub title: Option<String>,
    pub energy: Option<EnergyLevel>,
}

/// The same, once every field has been gathered.
pub enum Resolved {
    /// An existing bullet, named by the text that matched it.
    Bullet { project: String, query: String },
    /// A bullet to add and start.
    New {
        project: String,
        title: String,
        energy: EnergyLevel,
    },
}

impl Resolved {
    /// The `cdno` (plus `--vault`, when the person passed one) the
    /// suggested commands start with.
    fn cdno(vault_flag: Option<&Path>) -> String {
        match vault_flag {
            Some(v) => format!("cdno --vault {}", shell_word(&v.to_string_lossy())),
            None => "cdno".to_owned(),
        }
    }

    /// The exact `cdno action switch …` that does what this start meant.
    fn switch_command(&self, vault_flag: Option<&Path>) -> String {
        let cdno = Self::cdno(vault_flag);
        match self {
            Resolved::Bullet { project, query } => format!(
                "{cdno} action switch --project {} --query {}",
                shell_word(project),
                shell_word(query)
            ),
            Resolved::New {
                project,
                title,
                energy,
            } => format!(
                "{cdno} action switch --project {} --unplanned --title {} --energy {}",
                shell_word(project),
                shell_word(title),
                energy.as_str()
            ),
        }
    }

    /// What was tried, for the rejection's `details.attempted`.
    fn attempted(&self) -> serde_json::Value {
        match self {
            Resolved::Bullet { project, query } => {
                serde_json::json!({ "project": project, "query": query })
            }
            Resolved::New { project, title, .. } => {
                serde_json::json!({ "project": project, "title": title })
            }
        }
    }
}

/// `s` as one shell word: bare when it is plainly safe, else single-quoted.
fn shell_word(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'));
    if safe {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// The bullets the picker offers: every open bullet except the one in
/// focus, since switching to it is refused and offering it would be a
/// dead end.
pub fn pickable_labels(
    entries: &[ActionListEntry],
    project: &str,
    exclude: Option<&cdno_domain::CurrentFocus>,
) -> Vec<String> {
    entries
        .iter()
        .filter(|e| !exclude.is_some_and(|f| f.project == project && f.action == e.text))
        .map(|e| e.text.clone())
        .collect()
}

/// Gather what a start or switch acts on, through `gather_or_error`.
fn gather_target(
    vault: &Vault,
    target: Target,
    interactive: bool,
    prompted: &mut bool,
    exclude: Option<&cdno_domain::CurrentFocus>,
) -> Result<Resolved> {
    let project =
        prompt::gather_or_error(target.project, "project", interactive, prompted, || {
            prompt::prompt_project(vault)
        })?;
    if target.unplanned {
        let title = prompt::gather_or_error(target.title, "title", interactive, prompted, || {
            prompt::prompt_text("Title")
        })?;
        let energy =
            prompt::gather_or_error(target.energy, "energy", interactive, prompted, || {
                prompt::prompt_energy()
            })?;
        return Ok(Resolved::New {
            project,
            title,
            energy,
        });
    }
    let query = prompt::gather_or_error(target.query, "query", interactive, prompted, || {
        let entries = vault
            .list_actions(&project)
            .context("listing actions for the bullet picker")?;
        // Verbatim `text`, energy suffix and wikilink included: the
        // domain's whole-bullet tiebreak needs the exact string, and
        // `start_action` resolves through the same matcher the close
        // verbs use.
        prompt::prompt_bullet(&project, &pickable_labels(&entries, &project, exclude))
    })?;
    Ok(Resolved::Bullet { project, query })
}

/// A `FocusOpen` refusal, with its three facts.
struct Refusal {
    focus: cdno_domain::CurrentFocus,
    same_action: bool,
    carried: bool,
}

/// The question a refused `start` ends with. Its default, `false`, is
/// passed at the call site: Enter must never move focus.
pub const SWITCH_OFFER: &str = "Switch to it instead?";

/// `cdno action start` — two intents behind one verb, kept apart the
/// way the domain keeps them.
///
/// `--query` starts a bullet that already exists; `--unplanned` creates
/// one and starts it. They are `conflicts_with` at the clap level, so
/// the mode is never inferred. That separation is the whole point of
/// #568: routing a non-matching query into creation would turn a typo
/// into a new action, silently, which is the failure the domain change
/// removed.
///
/// With a focus already open the start is refused (RFC 0005 §5.1). The
/// refusal names the open focus and the exact `switch` that would do
/// what was asked; in a terminal it also offers to run it.
fn start(
    vault: &Vault,
    at: NaiveDateTime,
    target: Target,
    interactive: bool,
    json: bool,
    vault_flag: Option<&Path>,
) -> Result<()> {
    let mut prompted = false;
    let resolved = gather_target(vault, target, interactive, &mut prompted, None)?;
    start_after_gather(
        vault,
        at,
        resolved,
        prompted,
        interactive,
        json,
        vault_flag,
        prompt::confirm_preview,
        prompt::prompt_confirm,
    )
}

/// Whether a start shows its preview and asks to proceed. Not when a
/// focus is open: the start will be refused, and a question the answer to
/// which cannot matter is the wrong thing to ask first.
pub fn start_confirms(prompted: bool, focus_open: bool) -> bool {
    prompted && !focus_open
}

/// [`start`] from resolved arguments, with `confirm` (the preview) and
/// `offer` (the question, given its default) injected so a test can drive
/// it without a pty.
///
/// With `json` set a refusal prints the rejection object and **exits the
/// process** with status 1 (see `reject_json`); tests must not pass it.
#[allow(clippy::too_many_arguments)]
pub fn start_after_gather(
    vault: &Vault,
    at: NaiveDateTime,
    mut resolved: Resolved,
    prompted: bool,
    interactive: bool,
    json: bool,
    vault_flag: Option<&Path>,
    confirm: impl FnOnce(&str) -> Result<bool>,
    offer: impl FnOnce(&str, bool) -> Result<bool>,
) -> Result<()> {
    let focus_open = vault
        .current_focus(at.date())
        .context("reading the current focus")?
        .is_some();
    let confirms = start_confirms(prompted, focus_open);

    let refusal = match &mut resolved {
        Resolved::New {
            project,
            title,
            energy,
        } => {
            if confirms
                && !confirm(&format!(
                    "About to ADD to '{project}' and start it:\n  title:  {title}\n  energy: {}",
                    energy.as_str(),
                ))?
            {
                println!("Aborted.");
                return Ok(());
            }
            match vault.start_unplanned_action(at, project, title, *energy) {
                Ok(outcome) => {
                    let path = outcome.primary;
                    return crate::output::emit_write_result(
                        json,
                        &path.to_string(),
                        &format!("Added to {path} and started"),
                    );
                }
                Err(cdno_domain::error::DomainError::FocusOpen {
                    focus,
                    same_action,
                    carried,
                }) => Refusal {
                    focus,
                    same_action,
                    carried,
                },
                Err(e) => return Err(e).context("starting unplanned action"),
            }
        }
        Resolved::Bullet { project, query } => {
            if confirms && !confirm(&format!("About to START on '{project}': '{query}'"))? {
                println!("Aborted.");
                return Ok(());
            }
            // The text the call last ran with: after a picker round the
            // refusal's switch command must name the bullet that was chosen.
            let mut ran = query.clone();
            let result =
                resolving_ambiguity(project, query, interactive, "starting action", |q| {
                    q.clone_into(&mut ran);
                    match vault.start_action(at, project, q) {
                        Ok(daily) => Ok(Ok(daily)),
                        Err(cdno_domain::error::DomainError::FocusOpen {
                            focus,
                            same_action,
                            carried,
                        }) => Ok(Err(Refusal {
                            focus,
                            same_action,
                            carried,
                        })),
                        Err(e) => Err(e),
                    }
                })?;
            match result {
                Ok(daily) => {
                    return crate::output::emit_write_result(
                        json,
                        &daily.to_string(),
                        &format!("Started on {project}, logged to {daily}"),
                    );
                }
                Err(refusal) => {
                    *query = ran;
                    refusal
                }
            }
        }
    };
    refuse_start(
        vault,
        at,
        &resolved,
        refusal,
        interactive,
        json,
        vault_flag,
        offer,
    )
}

/// Report a refused start, and in a terminal offer the switch.
#[allow(clippy::too_many_arguments)]
fn refuse_start(
    vault: &Vault,
    at: NaiveDateTime,
    resolved: &Resolved,
    refusal: Refusal,
    interactive: bool,
    json: bool,
    vault_flag: Option<&Path>,
    offer: impl FnOnce(&str, bool) -> Result<bool>,
) -> Result<()> {
    if json {
        reject_json(focus_open_rejection(
            &refusal.focus,
            refusal.same_action,
            refusal.carried,
            Some(resolved.attempted()),
        ))?;
    }
    let text = refusal_text(resolved, &refusal, at.date(), vault_flag);
    // Switching to the bullet already in focus is itself refused, so
    // there is nothing to offer.
    if !interactive || refusal.same_action {
        anyhow::bail!(text);
    }
    println!("{text}");
    // The default is the literal `false`, here: Enter must not move focus.
    if !offer(SWITCH_OFFER, false)? {
        println!("Aborted.");
        return Ok(());
    }
    // The arguments are already resolved, so neither `--next` nor
    // `--reason` is asked for.
    run_switch(
        vault,
        at,
        resolved,
        None,
        None,
        interactive,
        json,
        vault_flag,
    )
}

/// What a refused start says: the open focus, then the way past it.
fn refusal_text(
    resolved: &Resolved,
    r: &Refusal,
    today: chrono::NaiveDate,
    vault_flag: Option<&Path>,
) -> String {
    use cdno_domain::FocusRemedy;
    let title = crate::output::sanitise(&r.focus.title());
    let project = &r.focus.project;
    match FocusRemedy::of(r.same_action, r.carried) {
        FocusRemedy::AlreadyFocused => {
            format!("{title} is already in focus on {project} \u{2014} nothing to do.")
        }
        FocusRemedy::Resume => format!(
            "{title} is already in focus on {project}, carried over from {}. \
             Pick it up again with `{} action resume`.",
            r.focus.date,
            Resolved::cdno(vault_flag)
        ),
        FocusRemedy::Switch => {
            // Worded like `cdno now`, so a carried focus names its day.
            let since = crate::commands::now::when(
                chrono::NaiveDateTime::new(r.focus.date, r.focus.started),
                today,
            );
            format!(
                "{title} is already in focus on {project} (since {since}).\n\
                 To move on to this instead: {}\n\
                 Or complete or pause it first.",
                resolved.switch_command(vault_flag)
            )
        }
    }
}

/// `cdno action switch` — pause what is open and start the target, in
/// one commit (RFC 0005 §5.1).
#[allow(clippy::too_many_arguments)] // thin gather→confirm→execute passthrough
fn switch(
    vault: &Vault,
    at: NaiveDateTime,
    target: Target,
    next: Option<String>,
    reason: Option<String>,
    interactive: bool,
    json: bool,
    vault_flag: Option<&Path>,
) -> Result<()> {
    let focus = vault
        .current_focus(at.date())
        .context("reading the current focus")?;
    let mut prompted = false;
    let resolved = gather_target(vault, target, interactive, &mut prompted, focus.as_ref())?;
    switch_after_gather(
        vault,
        at,
        resolved,
        prompted,
        next,
        reason,
        interactive,
        json,
        vault_flag,
        prompt::confirm_preview,
        || prompt::prompt_text("Where to pick up (Enter to skip)"),
    )
}

/// [`switch`] from resolved arguments, with the confirm and the hint
/// question injected (see [`pause_asking`]).
///
/// With `json` set a refusal prints the rejection object and **exits the
/// process** (see `reject_json`); tests must not pass it.
#[allow(clippy::too_many_arguments)]
pub fn switch_after_gather(
    vault: &Vault,
    at: NaiveDateTime,
    resolved: Resolved,
    prompted: bool,
    next: Option<String>,
    reason: Option<String>,
    interactive: bool,
    json: bool,
    vault_flag: Option<&Path>,
    confirm: impl FnOnce(&str) -> Result<bool>,
    ask: impl FnOnce() -> Result<String>,
) -> Result<()> {
    let focus = vault
        .current_focus(at.date())
        .context("reading the current focus")?;
    if prompted && !confirm(&switch_preview(focus.as_ref(), &resolved, &next))? {
        println!("Aborted.");
        return Ok(());
    }

    // The hint is asked for after the confirm and adds nothing to it. With
    // nothing open there is nothing to attach it to, so it is not asked.
    let next = if focus.is_some() {
        pause_hint(next, interactive, ask)?
    } else {
        next
    };
    run_switch(
        vault,
        at,
        &resolved,
        next.as_deref(),
        reason.as_deref(),
        interactive,
        json,
        vault_flag,
    )
}

/// What a prompted switch shows before it asks to proceed: both sides,
/// and a typed `--next`. A hint still to be asked for is not here — that
/// question comes after the confirm and adds nothing to it.
pub fn switch_preview(
    focus: Option<&cdno_domain::CurrentFocus>,
    resolved: &Resolved,
    next: &Option<String>,
) -> String {
    let starting = match resolved {
        Resolved::Bullet { project, query } => format!("start '{query}' on '{project}'"),
        Resolved::New {
            project,
            title,
            energy,
        } => format!(
            "ADD '{title}' ({}) to '{project}' and start it",
            energy.as_str()
        ),
    };
    match focus {
        Some(f) => {
            let mut p = format!(
                "About to pause {} on '{}', {starting}",
                crate::output::sanitise(&f.title()),
                f.project
            );
            if let Some(n) = next {
                p.push_str(&format!("\n  next:  {}", crate::output::sanitise(n)));
            }
            p
        }
        None => format!("Nothing is open. About to {starting}"),
    }
}

/// Run the switch for resolved arguments and report it.
#[allow(clippy::too_many_arguments)]
fn run_switch(
    vault: &Vault,
    at: NaiveDateTime,
    resolved: &Resolved,
    next: Option<&str>,
    reason: Option<&str>,
    interactive: bool,
    json: bool,
    vault_flag: Option<&Path>,
) -> Result<()> {
    use cdno_domain::error::DomainError;
    let attempt = match resolved {
        Resolved::New {
            project,
            title,
            energy,
        } => vault.switch_unplanned_action(at, project, title, *energy, next, reason),
        Resolved::Bullet { project, query } => {
            resolving_ambiguity(project, query, interactive, "switching action", |q| {
                match vault.switch_action(at, project, q, next, reason) {
                    // Handed back as a value so the refusal keeps its type.
                    Err(e @ DomainError::FocusOpen { .. }) => Ok(Err(e)),
                    Err(e) => Err(e),
                    Ok(o) => Ok(Ok(o)),
                }
            })?
        }
    };
    let outcome = match attempt {
        Ok(o) => o,
        Err(DomainError::FocusOpen {
            focus,
            same_action,
            carried,
        }) => {
            // Only the bullet already in focus is refused by a switch.
            let r = Refusal {
                focus,
                same_action,
                carried,
            };
            if json {
                reject_json(focus_open_rejection(
                    &r.focus,
                    same_action,
                    carried,
                    Some(resolved.attempted()),
                ))?;
            }
            anyhow::bail!(refusal_text(resolved, &r, at.date(), vault_flag));
        }
        Err(e) => return Err(e).context("switching action"),
    };
    let started = crate::output::sanitise(&outcome.started.title());
    // The log lines are in the daily note; for an unplanned switch the
    // primary path is the project map, so name the note the log is in.
    let daily = outcome
        .paths
        .iter()
        .find(|p| {
            p.to_string()
                .starts_with(&format!("{}/", cdno_core::paths::JOURNAL))
        })
        .unwrap_or(&outcome.primary);
    let message = match &outcome.paused {
        Some(p) => format!(
            "Paused {} on {}, started {started} on {}, logged to {daily}",
            crate::output::sanitise(&p.title()),
            p.project,
            outcome.started.project,
        ),
        None => {
            let mut m = format!("Nothing was open \u{2014} started {started}.");
            if next.is_some() {
                m.push_str("\n(--next ignored: nothing to attach it to)");
            }
            m
        }
    };
    crate::output::emit_write_result(json, &outcome.primary.to_string(), &message)
}

/// Run an action verb that resolves its target by substring, turning an
/// ambiguous match into a question rather than a dead end.
///
/// `AmbiguousAction` carries its candidates as a `Vec<String>`, and a verb
/// that hands the error straight to anyhow prints them as a Rust debug vec
/// — `["Run sweep B", "Run sweep C"]` — inside an error chain. Every verb
/// that resolves this way routes through here instead: a picker when a
/// terminal can show one, a listed set otherwise.
///
/// `call` takes the query so it can be re-run with the candidate the user
/// picked; `context` is the anyhow context for any *other* domain error,
/// and stays per-verb so "completing action" does not become "resolving
/// action" in the one place a user reads it.
fn resolving_ambiguity<T>(
    project: &str,
    query: &str,
    interactive: bool,
    context: &'static str,
    mut call: impl FnMut(&str) -> std::result::Result<T, cdno_domain::error::DomainError>,
) -> Result<T> {
    match call(query) {
        Ok(value) => Ok(value),
        Err(cdno_domain::error::DomainError::AmbiguousAction { candidates, .. }) => {
            if interactive && prompt::picker_fits(crate::output::terminal_columns()) {
                // The candidates are already known, so offer exactly
                // those. A whole bullet usually resolves uniquely on the
                // second call, via the exact-match tiebreak — but not
                // when two bullets' text differs only in case, or not
                // at all -- `action add` allows both freely, and the
                // tiebreak compares lowercased. Then it sees two exact
                // matches, declines, and the substring rule
                // re-ambiguates.
                let chosen = prompt::prompt_bullet(project, &candidates)?;
                return resolve_chosen(project, &chosen, &candidates, context, call);
            }
            anyhow::bail!(ambiguous_message(project, query, &candidates))
        }
        Err(e) => Err(e).context(context),
    }
}

/// Re-run the verb for the candidate the user picked out of the picker.
///
/// Split out so a test can reach it without driving a pty. The second call
/// can itself be ambiguous — two bullets whose text differs only in case
/// (or not at all) defeat the domain's whole-bullet tiebreak, which
/// compares lowercased and so sees two EXACT matches and declines — and
/// that error must land in the same readable
/// message rather than escaping through `.context` as the debug vec this
/// whole path exists to remove.
pub fn resolve_chosen<T>(
    project: &str,
    chosen: &str,
    candidates: &[String],
    context: &'static str,
    mut call: impl FnMut(&str) -> std::result::Result<T, cdno_domain::error::DomainError>,
) -> Result<T> {
    match call(chosen) {
        Ok(value) => Ok(value),
        Err(cdno_domain::error::DomainError::AmbiguousAction { .. }) => {
            anyhow::bail!(ambiguous_message(project, chosen, candidates))
        }
        Err(e) => Err(e).context(context),
    }
}

/// The candidates, one per line, instead of a Rust debug vec. Shared by
/// both ambiguity exits so they cannot drift apart.
///
/// Each candidate is note-derived bullet text, so it goes through
/// [`crate::output::sanitise`] like every other such string the CLI
/// lays out (`render_list` below does the same). The debug vec this
/// replaces escaped control characters as a side effect of `{:?}`;
/// printing the candidates plainly would have been a regression on
/// that, letting a bullet drive the terminal on the one path that
/// exists to make the error readable.
fn ambiguous_message(project: &str, query: &str, candidates: &[String]) -> String {
    format!(
        "ambiguous action match for '{query}' on project '{project}'. Candidates:\n{}",
        candidates
            .iter()
            .map(|c| format!("  {}", crate::output::sanitise(c)))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

#[allow(clippy::too_many_arguments)] // thin gather→create passthrough
fn promote(
    vault: &Vault,
    at: NaiveDateTime,
    project: Option<String>,
    query: Option<String>,
    var: Vec<(String, String)>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let project = prompt::gather_or_error(project, "project", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let query = prompt::gather_or_error(query, "query", interactive, &mut prompted, || {
        let entries = vault
            .list_actions(&project)
            .context("listing actions for the bullet picker")?;
        let labels: Vec<String> = entries.iter().map(|e| e.text.clone()).collect();
        let picked = prompt::prompt_bullet(&project, &labels)?;
        // The picker holds the bullet verbatim, and verbatim is what the
        // domain resolves most precisely: an exact whole-bullet match wins
        // outright, which is the only way to tell two bullets apart when
        // they differ solely by energy. Stripping here defeated that.
        Ok(picked)
    })?;
    // Promotion scaffolds an action note, so it gathers the action template's
    // prompted variables just like `add --note`.
    let template_vars =
        prompt::gather_template_vars(vault, "action", None, &var, interactive, &mut prompted)?;

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to promote action on '{project}': '{query}'"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let note_path = resolving_ambiguity(&project, &query, interactive, "promoting action", |q| {
        vault.promote_action_with_vars(at, &project, q, &template_vars)
    })?;
    crate::output::emit_write_result(
        json,
        &note_path.to_string(),
        &format!("Promoted to {note_path}"),
    )?;
    Ok(())
}

fn complete(
    vault: &Vault,
    at: NaiveDateTime,
    project: Option<String>,
    query: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let project = prompt::gather_or_error(project, "project", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let query = prompt::gather_or_error(query, "query", interactive, &mut prompted, || {
        let entries = vault
            .list_actions(&project)
            .context("listing actions for the bullet picker")?;
        let labels: Vec<String> = entries.iter().map(|e| e.text.clone()).collect();
        let picked = prompt::prompt_bullet(&project, &labels)?;
        // The picker holds the bullet verbatim, and verbatim is what the
        // domain resolves most precisely: an exact whole-bullet match wins
        // outright, which is the only way to tell two bullets apart when
        // they differ solely by energy. Stripping here defeated that.
        Ok(picked)
    })?;

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to complete action on '{project}': '{query}'"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    // The CLI reports the primary path only; the outcome's full
    // touched-path set is desktop-journal machinery (#315).
    let project_path =
        resolving_ambiguity(&project, &query, interactive, "completing action", |q| {
            vault.complete_action(at, &project, q)
        })?
        .primary;
    crate::output::emit_write_result(
        json,
        &project_path.to_string(),
        &format!("Action done on {project_path}"),
    )?;
    Ok(())
}

/// `cdno action drop` — the sibling of `complete`, for work that was
/// closed without being done (#559).
///
/// Named `drop_verb` because `drop` is a prelude function; the clap
/// variant is still `Drop` and the user-facing verb is still
/// `cdno action drop`.
///
/// `--reason` is genuinely optional and never prompted for: absence is a
/// valid value ("no reason recorded"), not a missing input, so it does
/// not route through `gather_or_error` (see the genuinely-optional-field
/// rule in `docs/cli-ergonomics.md`). It appears in the confirm preview
/// so a prompted run still shows what will be written.
fn drop_verb(
    vault: &Vault,
    at: NaiveDateTime,
    project: Option<String>,
    query: Option<String>,
    reason: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let project = prompt::gather_or_error(project, "project", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let query = prompt::gather_or_error(query, "query", interactive, &mut prompted, || {
        let entries = vault
            .list_actions(&project)
            .context("listing actions for the bullet picker")?;
        let labels: Vec<String> = entries.iter().map(|e| e.text.clone()).collect();
        prompt::prompt_bullet(&project, &labels)
    })?;

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to DROP action on '{project}' (not complete it): '{query}'\n  reason: {}",
            reason.as_deref().unwrap_or("(none)")
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    // The CLI reports the primary path only; the outcome's full
    // touched-path set is desktop-journal machinery (#315).
    let project_path =
        resolving_ambiguity(&project, &query, interactive, "dropping action", |q| {
            vault.drop_action(at, &project, q, reason.as_deref())
        })?
        .primary;
    crate::output::emit_write_result(
        json,
        &project_path.to_string(),
        &format!("Action dropped on {project_path}"),
    )?;
    Ok(())
}

/// `cdno action pause` — stops the one focus there is (RFC 0005 §5.1).
///
/// Nothing here is gathered through `gather_or_error`: there is no
/// project or query to ask for. The one prompt is the re-entry hint,
/// skippable, and it deliberately neither sets `prompted` nor leads to
/// a confirm (see "What is not part of the convention" in
/// `docs/cli-ergonomics.md`). `--reason` is never prompted.
fn pause(
    vault: &Vault,
    at: NaiveDateTime,
    next: Option<String>,
    reason: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    pause_asking(vault, at, next, reason, interactive, json, || {
        prompt::prompt_text("Where to pick up (Enter to skip)")
    })
}

/// [`pause`] with the hint question injected, so a test can drive the
/// handler as an interactive run and count the questions without a pty.
pub fn pause_asking(
    vault: &Vault,
    at: NaiveDateTime,
    next: Option<String>,
    reason: Option<String>,
    interactive: bool,
    json: bool,
    ask: impl FnOnce() -> Result<String>,
) -> Result<()> {
    // Refuse before asking for a hint nobody can use.
    if vault
        .current_focus(at.date())
        .context("reading the current focus")?
        .is_none()
    {
        return no_focus(json, NO_FOCUS_TO_PAUSE.to_owned());
    }
    let next = pause_hint(next, interactive, ask)?;
    let outcome = match vault.pause_action(at, next.as_deref(), reason.as_deref()) {
        Ok(o) => o,
        Err(cdno_domain::error::DomainError::NoFocus) => {
            return no_focus(json, NO_FOCUS_TO_PAUSE.to_owned());
        }
        Err(e) => return Err(e).context("pausing action"),
    };
    crate::output::emit_write_result(
        json,
        &outcome.path.to_string(),
        &format!(
            "Paused on {}, logged to {}",
            outcome.paused.project, outcome.path
        ),
    )
}

/// The re-entry hint for a pause: the flag when given, else (in a
/// terminal only) whatever `ask` returns, with blank meaning none.
///
/// Public so a test can count the questions without a pty. It takes no
/// `prompted` flag on purpose: this question never leads to a confirm.
pub fn pause_hint(
    next: Option<String>,
    interactive: bool,
    ask: impl FnOnce() -> Result<String>,
) -> Result<Option<String>> {
    match next {
        Some(n) => Ok(Some(n)),
        None if interactive => {
            let typed = ask()?;
            let typed = typed.trim();
            Ok((!typed.is_empty()).then(|| typed.to_owned()))
        }
        None => Ok(None),
    }
}

const NO_FOCUS_TO_PAUSE: &str = "Nothing started \u{2014} nothing to pause.";

/// `cdno action resume` — re-anchors a carried focus or reopens a pause.
fn resume(vault: &Vault, at: NaiveDateTime, project: Option<String>, json: bool) -> Result<()> {
    use cdno_domain::ResumedKind;
    use cdno_domain::error::DomainError;

    let outcome = match vault.resume_action(at, project.as_deref()) {
        Ok(o) => o,
        Err(DomainError::NoFocus) => {
            return no_focus(
                json,
                match project.as_deref().map(str::trim) {
                    Some(p) => format!(
                        "Nothing to resume on {p} \u{2014} no carried focus or pause there."
                    ),
                    None => {
                        "Nothing to resume \u{2014} nothing is carried over or paused.".to_owned()
                    }
                },
            );
        }
        Err(DomainError::FocusOpen {
            focus,
            same_action,
            carried,
        }) => {
            if json {
                reject_json(focus_open_rejection(&focus, same_action, carried, None))?;
            }
            if same_action {
                anyhow::bail!(
                    "{} is already in focus on {} \u{2014} nothing to resume.",
                    crate::output::sanitise(&focus.title()),
                    focus.project,
                );
            }
            anyhow::bail!(
                "{} is already in focus on {}. Pause it first (`cdno action pause`), \
                 or move on with `cdno action switch`.",
                crate::output::sanitise(&focus.title()),
                focus.project,
            );
        }
        Err(e) => return Err(e).context("resuming action"),
    };
    let message = format!(
        "Resumed on {}, logged to {}",
        outcome.resumed.project, outcome.path
    );
    let from = &outcome.from;
    if json {
        let payload = serde_json::json!({
            "path": outcome.path.to_string(),
            "message": message,
            "resumed_from": {
                "kind": match from.kind {
                    ResumedKind::Carried => "carried",
                    ResumedKind::Paused => "paused",
                },
                "date": from.date.to_string(),
                "next": from.next,
                "reason": from.reason,
            },
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    println!("{message}");
    if let Some(next) = &from.next {
        println!("next: {}", crate::output::sanitise(next));
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Refusals as JSON (RFC 0005 §5.1)
// ---------------------------------------------------------------------

/// The `remedy` a `FocusOpen` carries: what the caller does next.
/// `already_focused` (the same action, started today), `resume_action`
/// (the same action, carried from an earlier day), else `switch_action`.
pub fn focus_open_remedy(same_action: bool, carried: bool) -> &'static str {
    cdno_domain::FocusRemedy::of(same_action, carried).as_str()
}

/// The `focus_open` rejection object. The one place the CLI builds it:
/// `crates/cdno-mcp/tests/fixtures/focus_open_rejection.json` is the
/// shape the MCP server's rejection is compared against too, so the two
/// surfaces cannot say it differently.
pub fn focus_open_rejection(
    focus: &cdno_domain::CurrentFocus,
    same_action: bool,
    carried: bool,
    attempted: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut object = serde_json::json!({
        "code": "focus_open",
        "message": "An action is already in focus. Ask the person before switching; do not retry.",
        "details": {
            "focus": {
                "project": focus.project,
                "action": focus.action,
                "started": focus.started.format("%H:%M").to_string(),
                "date": focus.date.to_string(),
                "carried": carried,
            },
            "same_action": same_action,
            "remedy": focus_open_remedy(same_action, carried),
        },
    });
    // What the caller tried; absent for `resume`, which names no target.
    if let Some(a) = attempted {
        object["details"]["attempted"] = a;
    }
    object
}

/// The `no_focus` rejection object.
pub fn no_focus_rejection() -> serde_json::Value {
    serde_json::json!({
        "code": "no_focus",
        "message": "Nothing is started.",
        "details": {},
    })
}

/// Print a rejection object and exit non-zero, as `project complete`
/// does for its refusal: a script checking the exit code must not read
/// a refusal as a pass. Returns only if printing failed.
fn reject_json(value: serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    std::process::exit(1);
}

/// "Nothing is open to act on": the `no_focus` object under `--json`,
/// else `text` as the error.
fn no_focus(json: bool, text: String) -> Result<()> {
    if json {
        reject_json(no_focus_rejection())?;
    }
    anyhow::bail!(text)
}

fn list(vault: &Vault, project: Option<String>, interactive: bool, json: bool) -> Result<()> {
    // List is read-only — no confirm step even if we prompt for the
    // project, since nothing is being mutated.
    let project = match project {
        Some(p) => p,
        None if interactive => prompt::prompt_project(vault)?,
        None => return Err(prompt::missing_flag("project")),
    };
    let entries = vault.list_actions(&project).context("listing actions")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        print!("{}", render_list(&project, &entries));
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Shared gather helper and small utilities.
// ---------------------------------------------------------------------

fn yesno(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// Render `cdno action list` output. Pure so tests can exercise the
/// formatting without going through stdout.
pub fn render_list(project: &str, entries: &[ActionListEntry]) -> String {
    // Bullets, not cards: an action is one short line, so a gutter and a
    // header per item would cost three lines to say what one already
    // says. Colour carries the status instead.
    let palette = Palette::active();
    let title = palette.paint(Role::Heading, &format!("Actions for projects/{project}.md"));
    if entries.is_empty() {
        // Empty states hug their title everywhere; the blank line
        // separates a title from *content*.
        let mut out = format!("{title}\n");
        out.push_str(&format!(
            "  {}\n",
            palette.paint(Role::Muted, "(no open actions)")
        ));
        return out;
    }
    let mut out = format!("{title}\n\n");
    for entry in entries {
        out.push_str("  - ");
        out.push_str(&crate::output::sanitise(&entry.text));
        if let Some(att) = &entry.attached {
            let label = status_label(att);
            let role = status_role(att.status);
            out.push_str(&format!("  {}", palette.paint(role, &format!("[{label}]"))));
        }
        out.push('\n');
    }
    out
}

fn status_label(att: &AttachedAction) -> &'static str {
    match att.status {
        ActionStatus::Active => "active",
        ActionStatus::Blocked => "blocked",
        ActionStatus::Completed => "completed",
        ActionStatus::Dropped => "dropped",
    }
}

/// The style an action's status reads in.
///
/// Named rather than inlined so the mapping can be asserted: a test
/// that reads only the literal `[blocked]` text cannot see two roles
/// collapse into one. `a_rendered_listing_actually_uses_the_status_role`
/// renders under `with_colour(true, ..)` and compares SGR sequences, so
/// collapsing `Blocked` into `Meta` fails there and in
/// `action_statuses_are_distinguishable_in_the_rendered_listing`.
///
/// The mapping is deliberately not injective: `Active` and `Dropped`
/// both read as `Role::Meta`, so colour does not separate that pair.
/// What colour carries is the claim of achievement — only a real
/// completion reads as `Success` — and that is what the tests pin.
pub fn status_role(status: ActionStatus) -> Role {
    match status {
        ActionStatus::Active => Role::Meta,
        ActionStatus::Blocked => Role::Warn,
        ActionStatus::Completed => Role::Success,
        // Neutral, not Success: a dropped action is closed, but nothing
        // was achieved, and colouring it like a completion is the same
        // false claim in a different medium.
        ActionStatus::Dropped => Role::Meta,
    }
}
