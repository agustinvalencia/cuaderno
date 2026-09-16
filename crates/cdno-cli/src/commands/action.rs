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
    /// `action promote` rewrites the bullet it matches, so promoting
    /// between a start and its close strands the focus for the rest of
    /// the day and the close verbs then match nothing.
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
            project,
            query,
            unplanned,
            title,
            energy,
            interactive,
            json,
        ),
        ActionCommands::Complete { project, query } => {
            complete(&vault, at, project, query, interactive, json)
        }
        ActionCommands::Drop {
            project,
            query,
            reason,
        } => drop_verb(&vault, at, project, query, reason, interactive, json),
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

/// `cdno action start` — two intents behind one verb, kept apart the
/// way the domain keeps them.
///
/// `--query` starts a bullet that already exists; `--unplanned` creates
/// one and starts it. They are `conflicts_with` at the clap level, so
/// the mode is never inferred. That separation is the whole point of
/// #568: routing a non-matching query into creation would turn a typo
/// into a new action, silently, which is the failure the domain change
/// removed.
#[allow(clippy::too_many_arguments)] // two modes, each a gather→execute passthrough
fn start(
    vault: &Vault,
    at: NaiveDateTime,
    project: Option<String>,
    query: Option<String>,
    unplanned: bool,
    title: Option<String>,
    energy: Option<EnergyLevel>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let project = prompt::gather_or_error(project, "project", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;

    if unplanned {
        let title = prompt::gather_or_error(title, "title", interactive, &mut prompted, || {
            prompt::prompt_text("Title")
        })?;
        let energy = prompt::gather_or_error(energy, "energy", interactive, &mut prompted, || {
            prompt::prompt_energy()
        })?;
        if prompted
            && !prompt::confirm_preview(&format!(
                "About to ADD to '{project}' and start it:\n  title:  {title}\n  energy: {}",
                energy.as_str(),
            ))?
        {
            println!("Aborted.");
            return Ok(());
        }
        let path = vault
            .start_unplanned_action(at, &project, &title, energy)
            .context("starting unplanned action")?
            .primary;
        crate::output::emit_write_result(
            json,
            &path.to_string(),
            &format!("Added to {path} and started"),
        )?;
        return Ok(());
    }

    let query = prompt::gather_or_error(query, "query", interactive, &mut prompted, || {
        let entries = vault
            .list_actions(&project)
            .context("listing actions for the bullet picker")?;
        // Verbatim `text`, energy suffix and wikilink included: the
        // domain's whole-bullet tiebreak needs the exact string, and
        // `start_action` resolves through the same matcher the close
        // verbs use.
        let labels: Vec<String> = entries.iter().map(|e| e.text.clone()).collect();
        prompt::prompt_bullet(&project, &labels)
    })?;

    if prompted && !prompt::confirm_preview(&format!("About to START on '{project}': '{query}'"))? {
        println!("Aborted.");
        return Ok(());
    }

    let daily = resolving_ambiguity(&project, &query, interactive, "starting action", |q| {
        vault.start_action(at, &project, q)
    })?;
    crate::output::emit_write_result(
        json,
        &daily.to_string(),
        &format!("Started on {project}, logged to {daily}"),
    )?;
    Ok(())
}

/// Call `start_action`, turning an ambiguous match into a question
/// rather than a dead end — the shape `cdno open` already uses for an
/// ambiguous note reference.
///
/// Without this the candidates reach the user only as a Rust `{:?}` vec
/// inside an anyhow chain, because `AmbiguousAction` carries them as a
/// `Vec<String>` and nothing in the CLI unpacks it.
///
/// `start` is the first CLI verb to *unpack* it, not the first that can
/// raise it: `complete`, `drop` and `promote` all resolve through the
/// same `resolve_open_action` (cdno-domain `vault/projects/actions.rs`,
/// a private free function, so no intra-doc link) and could raise it
/// before this verb
/// existed. They still hand it to anyhow, so they
/// still print the debug vec. Routing them through here too is worth
/// doing and is deliberately not done in the same change as adding the
/// verb.
/// Run an action verb that resolves its target by substring, turning an
/// ambiguous match into a question rather than a dead end.
///
/// `AmbiguousAction` carries its candidates as a `Vec<String>`, and a verb
/// that hands the error straight to anyhow prints them as a Rust debug vec
/// -- `["Run sweep B", "Run sweep C"]` -- inside an error chain. Every verb
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
                // second call, via the exact-match tiebreak -- but not
                // when two bullets carry byte-identical text, which
                // `action add` allows freely. Then the tiebreak sees two
                // exact matches, declines, and the substring rule
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
/// can itself be ambiguous -- two bullets carrying byte-identical text
/// defeat the domain's whole-bullet tiebreak, since it sees two EXACT
/// matches and declines -- and that error must land in the same readable
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
