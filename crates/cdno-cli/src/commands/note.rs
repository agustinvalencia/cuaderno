//! `cdno note` — create, list and revise notes of config-defined custom
//! types.
//!
//! Built-in types have their own verbs (`cdno project`, `cdno question`, …);
//! a custom type declared under `[note_types.<type>]` has none, so this one
//! generic command serves them all.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use clap::Subcommand;

use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::vault::{Change, RenamePlan, RenameWarning, ReviseOutcome, Revision};

use crate::bootstrap;
use crate::prompt;

#[derive(Debug, Subcommand)]
pub enum NoteCommands {
    /// Create a note of a config-defined custom type (declared under
    /// `[note_types.<type>]` in `.cuaderno/config.toml`).
    Create {
        /// The custom note type, e.g. `person`.
        note_type: String,
        /// The note's title; its slug becomes the filename.
        #[arg(long)]
        title: String,
        /// A frontmatter field, `name=value`. Repeatable. Each key must be a
        /// declared `required`/`optional` field of the type.
        #[arg(long = "field", value_parser = crate::prompt::parse_key_val)]
        field: Vec<(String, String)>,
        /// Value for a prompted template variable (`[variables.prompt]`),
        /// `name=value`. Repeatable.
        #[arg(long = "var", value_parser = crate::prompt::parse_key_val)]
        var: Vec<(String, String)>,
        /// A file whose contents become the note's body, without the title
        /// heading (the engine writes the H1; a leading `# <title>` line is
        /// dropped). It fills the type template's `{{body}}` placeholder, or
        /// is inserted after the H1 when the template has none. When the
        /// template has `{{body}}` and nothing else (`--field body=`,
        /// `--var body=`, a `[variables] body` in the config) fills it, it is
        /// prompted in an editor in an interactive session and required
        /// otherwise.
        #[arg(long = "body-file", value_name = "PATH")]
        body_file: Option<PathBuf>,
        /// Where the note came from: one string of wikilinks, e.g.
        /// `[[journal/2026/daily/2026-09-02#Woodbury identity]]` (promotion,
        /// RFC 0002). The type must declare an `origin` field.
        #[arg(long, value_name = "STRING")]
        origin: Option<String>,
        /// Slug for the note (its file name); derived from the title when
        /// omitted. Not combinable with `--field slug=…`.
        #[arg(long)]
        slug: Option<String>,
    },

    /// List every note of a config-defined custom type, by path.
    List {
        /// The custom note type.
        note_type: String,
    },

    /// Revise a mutable custom note (such as a concept) in place, logging
    /// `revised [[path]] — reason` to today's daily note. Built-in types and
    /// custom types declared `append_only = true` are refused.
    ///
    /// Give either `--body-file` (the whole body after the frontmatter) or
    /// `--section` with `--content-file` (upsert one section). With neither,
    /// an interactive session opens the current body in an editor.
    Revise {
        /// The note to revise, resolved as `cdno open` resolves a reference:
        /// a path with or without `.md`, a slug, or `type:slug`. Omitted in
        /// an interactive session, a picker offers every note.
        note: Option<String>,
        /// A file whose contents replace the note's whole body (everything
        /// after the frontmatter, which is kept as it is), written verbatim.
        #[arg(long = "body-file", value_name = "PATH", conflicts_with = "section")]
        body_file: Option<PathBuf>,
        /// The heading text of the section to upsert, without the `#`
        /// markers. An existing section is replaced together with its
        /// sub-sections; a missing one is appended as `## <section>`.
        #[arg(long, value_name = "STRING", requires = "content_file")]
        section: Option<String>,
        /// A file holding the section's new text, without its heading.
        #[arg(long = "content-file", value_name = "PATH", requires = "section")]
        content_file: Option<PathBuf>,
        /// Why the note was revised, in a short clause; it becomes the
        /// daily-log line `revised [[path]] — reason`.
        #[arg(long, value_name = "STRING")]
        reason: Option<String>,
    },

    /// Rename a note's slug and follow every reference to it: the links that
    /// resolve to it, the `project:` / `portfolio:` style fields that name it,
    /// an `aliases:` entry for the old slug on the note itself, and one
    /// `renamed` line in today's daily log (RFC 0007).
    ///
    /// Renamable: projects, questions, active action notes, portfolios and
    /// custom notes. A portfolio whose slug a question shares is renamed
    /// through the question, and the pair moves together. The preview (what
    /// moves, which notes are rewritten and how many references, a plan hash)
    /// is what gets written: confirm it, or run with `--dry-run` to see it
    /// and write nothing.
    Rename {
        /// The note to rename, resolved as `cdno open` resolves a reference:
        /// a path with or without `.md`, a slug, a former slug or `type:slug`.
        /// Omitted in an interactive session, a picker offers the renamable
        /// notes only.
        note: Option<String>,
        /// The new slug (the file name without `.md`). Prompted in an
        /// interactive session when omitted.
        #[arg(long, value_name = "SLUG")]
        to: Option<String>,
        /// Why the note is renamed, in a short clause; it becomes the
        /// `reason:` line under the log entry. Never prompted.
        #[arg(long, value_name = "STRING")]
        reason: Option<String>,
        /// Print the preview and exit 0, writing nothing.
        #[arg(long)]
        dry_run: bool,
        /// Accepted; takes effect with the freshness check.
        #[arg(long)]
        skip_sync_check: bool,
    },
}

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: NoteCommands,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result.
    let interactive = prompt::reports_interactively(no_interactive, json);
    match command {
        NoteCommands::Create {
            note_type,
            title,
            field,
            var,
            body_file,
            origin,
            slug,
        } => {
            let fields: HashMap<String, String> = field.into_iter().collect();
            let vars: HashMap<String, String> = var.into_iter().collect();
            let body_from_file = body_file
                .map(|p| {
                    std::fs::read_to_string(&p)
                        .with_context(|| format!("reading --body-file {}", p.display()))
                })
                .transpose()?;
            let mut prompted = false;
            // A body is gathered (prompted or demanded) only when the type's
            // template has a `{{body}}` slot that nothing else (a `body`
            // field, a `[variables] body`, a `--var body=`) fills; otherwise
            // it is optional.
            let body = if vault.custom_note_needs_body(&note_type, &fields, &vars)? {
                Some(prompt::gather_or_error(
                    body_from_file,
                    "body-file",
                    interactive,
                    &mut prompted,
                    || prompt::prompt_editor("Body", ""),
                )?)
            } else {
                body_from_file
            };
            if prompted
                && !prompt::confirm_preview(&format!(
                    "About to create {note_type} note:\n  title: {title}"
                ))?
            {
                println!("Aborted.");
                return Ok(());
            }
            let path = vault
                .create_custom_note_with_vars(
                    at,
                    &note_type,
                    &title,
                    &fields,
                    &vars,
                    body.as_deref(),
                    origin.as_deref(),
                    slug.as_deref(),
                )
                .map_err(anyhow::Error::from)
                .map_err(|e| crate::commands::rejection::slug_refusal(e, json))?;
            crate::output::emit_write_result(json, &path.to_string(), &format!("Created {path}"))
        }
        NoteCommands::Revise {
            note,
            body_file,
            section,
            content_file,
            reason,
        } => revise(
            root,
            &vault,
            at,
            ReviseArgs {
                note,
                body_file,
                section,
                content_file,
                reason,
            },
            interactive,
            json,
        ),
        NoteCommands::Rename {
            note,
            to,
            reason,
            dry_run,
            skip_sync_check: _,
        } => rename(
            root,
            &vault,
            at,
            RenameArgs {
                note,
                to,
                reason,
                dry_run,
            },
            interactive,
            json,
        ),
        NoteCommands::List { note_type } => {
            let paths = vault.list_custom_notes(&note_type)?;
            if json {
                let rows: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else if paths.is_empty() {
                println!("No `{note_type}` notes.");
            } else {
                for p in &paths {
                    println!("{p}");
                }
            }
            Ok(())
        }
    }
}

/// The flags of `cdno note revise`, bundled so [`revise`] stays readable.
struct ReviseArgs {
    note: Option<String>,
    body_file: Option<PathBuf>,
    section: Option<String>,
    content_file: Option<PathBuf>,
    reason: Option<String>,
}

/// `cdno note revise`: resolve, read, gather, confirm, revise.
///
/// The note is read first and its `content_hash` kept; that hash goes to
/// [`Vault::revise_note`] as `expected_hash`, so an edit that lands while
/// the editor is open (no lock is held meanwhile) is refused at commit
/// with the domain's `StaleRevision` message. The body or section file is
/// read only after the note, so what it holds is always measured against
/// the read the hash came from.
fn revise(
    root: &Path,
    vault: &Vault,
    at: NaiveDateTime,
    args: ReviseArgs,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    // A note chosen in a picker is a prompted value like any other (rule 3),
    // so the write confirms first: otherwise a mistyped slug would open the
    // all-notes picker and one keystroke would overwrite whatever was chosen.
    // `resolve_reporting` sets `prompted` when an ambiguity or a miss was
    // settled by the picker.
    let path = match args.note {
        Some(reference) => {
            let reference = crate::commands::open::strip_vault_root(&reference, root);
            crate::commands::open::resolve_reporting(
                vault,
                &reference,
                at.date(),
                interactive,
                &mut prompted,
            )?
        }
        None if interactive => {
            prompted = true;
            crate::commands::open::pick_from_all(vault, None)?
        }
        None => {
            return Err(prompt::missing_positional("note").context(
                "no note to revise — pass a reference (`cdno open --list` shows every note)",
            ));
        }
    };
    let view = vault.read_note(&path)?;

    let revision = match (args.section, args.content_file) {
        (Some(heading), Some(content_file)) => Revision::Section {
            // Headings are matched as trimmed text, so a padded heading
            // would miss the real section and be appended as a duplicate.
            heading: heading.trim().to_owned(),
            content: read_flag_file(&content_file, "content-file")?,
        },
        // clap's `requires` makes a half-given section pair unreachable.
        _ => {
            let body_from_file = args
                .body_file
                .map(|p| read_flag_file(&p, "body-file"))
                .transpose()?;
            Revision::Body(prompt::gather_or_error(
                body_from_file,
                "body-file",
                interactive,
                &mut prompted,
                || prompt::prompt_editor("Body", &view.body),
            )?)
        }
    };
    let reason =
        prompt::gather_or_error(args.reason, "reason", interactive, &mut prompted, || {
            prompt::prompt_text("Reason")
        })?;

    if prompted {
        let target = match &revision {
            Revision::Section { heading, .. } => format!("{path} (section `{heading}`)"),
            Revision::Body(_) => path.to_string(),
        };
        if !prompt::confirm_preview(&format!("About to revise {target}:\n  reason: {reason}"))? {
            println!("Aborted.");
            return Ok(());
        }
    }

    // The hash goes with a section revision too: the domain accepts it, and
    // refusing an edit that raced the read is the same promise either way.
    let outcome = vault.revise_note(
        &path,
        Some(view.content_hash.as_str()),
        revision,
        &reason,
        at,
    )?;
    emit_revise_result(json, &outcome)
}

/// Read a `--body-file` / `--content-file`, naming the flag on failure.
fn read_flag_file(path: &Path, flag: &str) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading --{flag} {}", path.display()))
}

/// Print a revision's result: `Revised <path>` or `No change to <path>`,
/// or with `--json` the `emit_write_result` shape (`path`, `message`)
/// extended with the [`ReviseOutcome`] fields.
fn emit_revise_result(json: bool, outcome: &ReviseOutcome) -> Result<()> {
    let message = if outcome.changed {
        format!("Revised {}", outcome.path)
    } else {
        format!("No change to {}", outcome.path)
    };
    if json {
        let payload = serde_json::json!({
            "path": outcome.path.to_string(),
            "message": message,
            "changed": outcome.changed,
            "new_hash": outcome.new_hash,
            "log_line": outcome.log_line,
            "section_target": outcome.section_target,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{message}");
    }
    Ok(())
}

/// The flags of `cdno note rename`.
struct RenameArgs {
    note: Option<String>,
    to: Option<String>,
    reason: Option<String>,
    dry_run: bool,
}

/// `cdno note rename`: resolve, gather, preview, confirm, rename.
///
/// The preview is the contract: [`Vault::rename_note`] is given the
/// preview's hash as `expected_plan`, so what is written is what was shown,
/// whether or not anything was prompted, and a vault that changed in between
/// is refused (`stale_plan`) rather than rewritten differently.
fn rename(
    root: &Path,
    vault: &Vault,
    at: NaiveDateTime,
    args: RenameArgs,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    // The reference goes to the domain as the person typed it, so a former
    // name is reported as one; only a picked note is passed as its path.
    let target = match args.note {
        Some(reference) => {
            let reference = crate::commands::open::strip_vault_root(&reference, root);
            let (path, _) = crate::commands::open::resolve_with_alias(
                vault,
                &reference,
                at.date(),
                interactive,
                &mut prompted,
            )?;
            if prompted {
                path.to_string()
            } else {
                reference
            }
        }
        None if interactive => {
            prompted = true;
            let renamable = renamable_filter(vault)?;
            crate::commands::open::pick_from_filtered(vault, None, &renamable)?.to_string()
        }
        None => {
            return Err(prompt::missing_positional("note").context(
                "no note to rename — pass a reference (`cdno open --list` shows every note)",
            ));
        }
    };
    let to = prompt::gather_or_error(args.to, "to", interactive, &mut prompted, || {
        prompt::prompt_text("New slug")
    })?;
    let reason = args.reason.as_deref();

    let plan = vault
        .rename_preview(&target, &to, reason, at)
        .map_err(|e| rename_refusal(e, json))?;
    if args.dry_run {
        if json {
            return emit_rename_json(&plan, &[], false, true);
        }
        println!("Dry run: nothing written.");
        print!("{}", render_plan(&plan));
        return Ok(());
    }
    if prompted && !plan.is_empty() && !prompt::confirm_preview(render_plan(&plan).trim_end())? {
        println!("Aborted.");
        return Ok(());
    }
    let outcome = vault
        .rename_note(&target, &to, reason, Some(plan.hash.as_str()), at)
        .map_err(|e| rename_refusal(e, json))?;
    if json {
        return emit_rename_json(&outcome.plan, &outcome.written, outcome.index_stale, false);
    }
    if outcome.plan.is_empty() {
        println!(
            "Nothing to do: `{}` already has the slug `{}`.",
            outcome.plan.target.slug, outcome.plan.new_slug
        );
        return Ok(());
    }
    if !prompted {
        print!("{}", render_plan(&outcome.plan));
    }
    println!(
        "Renamed `{}` to `{}`.",
        outcome.plan.target.slug, outcome.plan.new_slug
    );
    if outcome.index_stale {
        println!(
            "The files are written but the index is stale for some of them; the next command reconciles it."
        );
    }
    Ok(())
}

/// Which picker rows `cdno note rename` offers: the notes the domain would
/// not refuse as `note_not_renamable` (RFC 0007 §5.2): projects, questions,
/// active action notes, portfolio folder notes no question shares a slug
/// with, and custom notes. Dailies, weeklies, evidence, archived actions and
/// the other built-ins are not offered.
fn renamable_filter(
    vault: &Vault,
) -> Result<impl Fn(&cdno_core::index::NoteCandidate) -> bool + use<>> {
    use std::collections::HashSet;
    let candidates = vault
        .list_note_candidates()
        .context("listing notes in the vault")?;
    let question_slugs: HashSet<String> = candidates
        .iter()
        .filter(|c| c.note_type == "question")
        .map(|c| crate::commands::open::slug_of(&c.path))
        .collect();
    let custom: HashSet<String> = candidates
        .iter()
        .map(|c| c.note_type.clone())
        .filter(|name| {
            name.parse::<cdno_domain::note_type::NoteType>().is_err()
                && vault
                    .type_registry()
                    .resolve(name)
                    .is_some_and(|descriptor| descriptor.is_custom())
        })
        .collect();
    Ok(move |c: &cdno_core::index::NoteCandidate| {
        let path = c.path.as_path();
        match c.note_type.as_str() {
            "project" | "question" => true,
            "action" => !path.starts_with(cdno_core::paths::ACTIONS_DONE),
            "portfolio" => {
                path.file_name().is_some_and(|n| n == "_index.md")
                    && path.parent().and_then(Path::parent)
                        == Some(Path::new(cdno_core::paths::PORTFOLIOS))
                    && !question_slugs.contains(&crate::commands::open::slug_of(&c.path))
            }
            other => custom.contains(other),
        }
    })
}

/// A domain refusal of the rename, as the error the handler returns. Under
/// `--json` a refusal with a code is printed as its rejection object and the
/// process exits non-zero; in text a failed rollback lists its paths one per
/// line, and any other error is returned for the usual `Error: …` line.
pub fn rename_refusal(err: DomainError, json: bool) -> anyhow::Error {
    if json
        && let Some(object) = crate::commands::rejection::rename_rejection(&err)
        && let Err(printing) = crate::commands::action::reject_json(object)
    {
        return printing;
    }
    if let DomainError::RenameIncomplete { failed, source } = &err {
        let mut lines = format!("the rename failed part-way ({source}) and could not restore:");
        for path in failed {
            lines.push_str(&format!("\n  {path}"));
        }
        lines.push_str("\ncheck them by hand (version control has the previous content)");
        return anyhow::anyhow!(lines);
    }
    err.into()
}

/// The `--json` shape: the plan, then `written` (the paths written, empty on
/// a dry run and for a no-op), `index_stale` and `dry_run`.
fn emit_rename_json(
    plan: &RenamePlan,
    written: &[cdno_core::path::VaultPath],
    index_stale: bool,
    dry_run: bool,
) -> Result<()> {
    let changes = |changes: &[Change]| -> Vec<serde_json::Value> {
        changes
            .iter()
            .map(|c| match c {
                Change::Address { count } => serde_json::json!({ "kind": "links", "count": count }),
                Change::Field { key } => serde_json::json!({ "kind": "field", "key": key }),
                Change::Alias => serde_json::json!({ "kind": "alias" }),
                Change::LogLine => serde_json::json!({ "kind": "log_line" }),
            })
            .collect()
    };
    let warnings: Vec<serde_json::Value> = plan.warnings.iter().map(warning_json).collect();
    let payload = serde_json::json!({
        "dry_run": dry_run,
        "target": plan.target.path.to_string(),
        "from": plan.target.slug,
        "to": plan.new_slug,
        "found_by_former_name": plan.target.resolved_via_alias,
        "moves": plan.moves.iter().map(|(from, to)| serde_json::json!({
            "from": from.to_string(),
            "to": to.to_string(),
        })).collect::<Vec<_>>(),
        "edits": plan.edits.iter().map(|e| serde_json::json!({
            "path": e.path.to_string(),
            "changes": changes(&e.changes),
        })).collect::<Vec<_>>(),
        "references": reference_totals(plan).0,
        "notes": reference_totals(plan).1,
        "warnings": warnings,
        "plan_hash": plan.hash,
        "written": written.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "index_stale": index_stale,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn warning_json(warning: &RenameWarning) -> serde_json::Value {
    match warning {
        RenameWarning::FrozenPrefixAlreadyDiverged { path } => serde_json::json!({
            "kind": "frozen_prefix_already_diverged",
            "path": path.to_string(),
            "message": warning_text(warning),
        }),
        RenameWarning::OldSlugAlreadyAnAlias { holder } => serde_json::json!({
            "kind": "old_slug_already_an_alias",
            "holder": holder.to_string(),
            "message": warning_text(warning),
        }),
    }
}

fn warning_text(warning: &RenameWarning) -> String {
    match warning {
        RenameWarning::FrozenPrefixAlreadyDiverged { path } => format!(
            "{path} already failed its archival check before this rename; it is rewritten anyway and stays flagged"
        ),
        RenameWarning::OldSlugAlreadyAnAlias { holder } => format!(
            "the old slug is already an alias of {holder}; once this note lists it too the alias is shared and resolves to nothing"
        ),
    }
}

/// References rewritten (links plus fields) and the notes holding them.
fn reference_totals(plan: &RenamePlan) -> (usize, usize) {
    let mut references = 0;
    let mut notes = 0;
    for edit in &plan.edits {
        let here: usize = edit
            .changes
            .iter()
            .map(|c| match c {
                Change::Address { count } => *count,
                Change::Field { .. } => 1,
                Change::Alias | Change::LogLine => 0,
            })
            .sum();
        references += here;
        notes += usize::from(here > 0);
    }
    (references, notes)
}

/// The preview, RFC 0007 §1.1: a `move` line per move, a `rewrite` line per
/// note whose references change, the alias line, the log line, a totals line
/// ending in the plan hash's first four characters, then the warnings.
fn render_plan(plan: &RenamePlan) -> String {
    let mut out = String::new();
    if let Some(alias) = &plan.target.resolved_via_alias {
        out.push_str(&format!("found by its former name `{alias}`\n"));
    }
    if plan.is_empty() {
        out.push_str(&format!(
            "nothing to do: `{}` already has the slug `{}`\n",
            plan.target.slug, plan.new_slug
        ));
        return out;
    }
    for (from, to) in &plan.moves {
        out.push_str(&format!("  move     {from} -> {to}\n"));
    }
    let width = plan
        .edits
        .iter()
        .map(|e| e.path.to_string().len())
        .max()
        .unwrap_or(0);
    for edit in &plan.edits {
        let mut links = 0;
        let mut keys: Vec<String> = Vec::new();
        for change in &edit.changes {
            match change {
                Change::Address { count } => links += count,
                Change::Field { key } => keys.push(format!("{key}:")),
                Change::Alias | Change::LogLine => {}
            }
        }
        let mut parts: Vec<String> = Vec::new();
        if links > 0 {
            parts.push(format!("{links} link{}", plural(links)));
        }
        if !keys.is_empty() {
            parts.push(format!(
                "{} field{} ({})",
                keys.len(),
                plural(keys.len()),
                keys.join(", ")
            ));
        }
        if !parts.is_empty() {
            out.push_str(&format!(
                "  rewrite  {:width$}  {}\n",
                edit.path.to_string(),
                parts.join(" / ")
            ));
        }
    }
    if plan
        .edits
        .iter()
        .any(|e| e.changes.contains(&Change::Alias))
    {
        out.push_str(&format!(
            "  alias    {} gains `{}`\n",
            plan.moves.first().map_or(&plan.target.path, |(_, to)| to),
            plan.target.slug
        ));
    }
    for edit in &plan.edits {
        if edit.changes.contains(&Change::LogLine)
            && let Some(last) = edit.edits.last()
        {
            out.push_str(&format!("  log      {}\n", edit.path));
            for line in last.replacement.trim().lines() {
                out.push_str(&format!("             {line}\n"));
            }
        }
    }
    let (references, notes) = reference_totals(plan);
    out.push_str(&format!(
        "  {references} reference{} in {notes} note{} · plan {}…\n",
        plural(references),
        plural(notes),
        plan.hash.chars().take(4).collect::<String>()
    ));
    for warning in &plan.warnings {
        out.push_str(&format!("  warning: {}\n", warning_text(warning)));
    }
    out
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
