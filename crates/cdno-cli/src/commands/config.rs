//! `cdno config` subcommands: inspect, check, and edit `.cuaderno/config.toml`.
//!
//! Vault config was reachable only from the desktop app's Config view. The
//! desktop is being retired, so the capability moves here before it goes
//! (#597, #598) — and config is arguably the most CLI-shaped thing in the
//! repo: a text file you want to read, check, and edit.
//!
//! ## Why `edit` round-trips a copy instead of opening the file
//!
//! `Vault::save_config_raw` is a three-step gate — validate FIRST (so a
//! config that would not reopen never reaches the disk), then
//! compare-and-swap against the hash the read handed out (so a concurrent
//! hand-edit is refused rather than clobbered), then write verbatim.
//! Handing an editor `.cuaderno/config.toml` itself would route the write
//! around all three: the editor writes directly, and a typo would brick the
//! vault exactly as the desktop's never-brick guarantee was built to
//! prevent.
//!
//! So `edit` copies the config to a scratch file, opens *that*, and feeds
//! the result back through the gate. The scratch file is named
//! `config.toml` so an editor still applies TOML syntax highlighting, and a
//! rejected buffer is deliberately left on disk with its path printed —
//! losing someone's edit because it failed validation would be a worse bug
//! than the one the gate exists to stop.
//!
//! ## Why a detached editor is refused rather than tolerated
//!
//! [`crate::editor::Editor::spawn`] returns `None` when the work was handed
//! off — a GUI editor that returns immediately, or the platform default
//! handler. There is then no moment at which the buffer is known to be
//! written, so reading it back would race the human: at best a no-op, at
//! worst saving a half-typed config. `cdno open` can hand off happily
//! because nothing reads the file afterwards; this cannot, so it says so
//! and names the fix.
//!
//! ## Why these verbs never open the vault
//!
//! `Vault::new` validates the config before it hands back a `Vault`, so a
//! broken config means no vault at all — and a broken config is exactly
//! when you need to read, check and fix one. Opening the vault here would
//! make `cdno config validate` fail with `loading config.toml` and no line,
//! column or reason, which is worse than useless on the one input it exists
//! to diagnose.
//!
//! So these verbs build a bare [`FsVaultStore`] over the vault root and call
//! the store-level [`read_config_from`] / [`save_config_to`], which
//! `Vault::read_config_raw` and `Vault::save_config_raw` also delegate to.
//! One implementation of the gate, reachable without an index, a
//! reconciliation pass, or a config that parses.
//!
//! ## The raw-write caveat
//!
//! `save_config_raw` is one of the documented paths that bypasses
//! `VaultTransaction` AND the cross-process advisory lock (see CLAUDE.md).
//! That is deliberate — config is not an append-only note, and the
//! compare-and-swap covers the single-user case this is built for — but do
//! not add new raw-write callers on its precedent.

use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Subcommand;

use cdno_core::config::{CustomNoteType, FieldSpec, FieldType, PlotKind, VaultConfig};
use cdno_core::config_edit;
use cdno_core::error::ConfigEditError;
use cdno_core::store::FsVaultStore;
use cdno_domain::vault::config::{read_config_from, save_config_to};
use cdno_domain::{ConfigSaveError, validate_config_str};

use crate::prompt::{gather_or_error, prompt_text};

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    /// Print `.cuaderno/config.toml` verbatim — comments, key order and
    /// the `[variables]` block exactly as they sit on disk. `--json` adds
    /// the content hash, which is what a compare-and-swap save checks.
    Show,

    /// Check that the config on disk would open, without changing
    /// anything. Runs the exact validation `Vault::new` performs, so a
    /// pass here means the vault opens. Exits non-zero on a bad config.
    Validate {
        /// Validate this file instead of the vault's own config. Useful
        /// for checking a candidate before putting it in place.
        #[arg(long, value_name = "PATH")]
        file: Option<std::path::PathBuf>,
    },

    /// Open the config in your editor, then save it through the same
    /// validate-first, compare-and-swap gate the desktop used — so a
    /// config that would not reopen is never written.
    Edit {
        /// Editor command to use, overriding `$VISUAL` / `$EDITOR`.
        /// Must wait for the file to be closed: a detached editor cannot
        /// be read back.
        #[arg(long, value_name = "COMMAND")]
        editor: Option<String>,
    },

    /// Add, change or remove a custom note type (`[note_types.<name>]`).
    NoteType {
        #[command(subcommand)]
        subcommand: NoteTypeCommands,
    },

    /// Add, change or remove a schema field
    /// (`[schemas.<type>.fields.<field>]`).
    Field {
        #[command(subcommand)]
        subcommand: FieldCommands,
    },

    /// Set how a tracking metric is plotted
    /// (`[tracking.<activity>.metrics.<metric>]`).
    Plot {
        #[command(subcommand)]
        subcommand: PlotCommands,
    },

    /// Add, change or remove a static template variable (`[variables]`).
    Var {
        #[command(subcommand)]
        subcommand: VarCommands,
    },

    /// Add, change or remove a prompted template variable
    /// (`[variables.prompt]`).
    Prompt {
        #[command(subcommand)]
        subcommand: PromptCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum NoteTypeCommands {
    /// Create a note type, or change one in place.
    ///
    /// Changing is a MERGE: a flag you do not pass keeps its current
    /// value. `set_note_type` itself replaces the whole table, so passing
    /// only `--folder` at that layer would silently drop the type's
    /// `required` list — the merge is what makes a one-flag edit safe.
    /// Pass an empty value to clear an optional key
    /// (`--template ''`, `--required ''`).
    Set {
        /// Name of the note type, i.e. `[note_types.<name>]`.
        #[arg(long)]
        name: Option<String>,
        /// Vault-relative folder its notes live in, e.g. `people`.
        /// Required when creating; preserved when omitted on an edit.
        #[arg(long)]
        folder: Option<String>,
        /// Comma-separated frontmatter fields that must be present.
        #[arg(long, value_name = "FIELDS")]
        required: Option<String>,
        /// Comma-separated frontmatter fields that may be present.
        #[arg(long, value_name = "FIELDS")]
        optional: Option<String>,
        /// Template filename under `.cuaderno/templates/`.
        #[arg(long, value_name = "FILE")]
        template: Option<String>,
        /// Mark notes of this type append-only.
        #[arg(long, conflicts_with = "no_append_only")]
        append_only: bool,
        /// Clear the append-only mark.
        #[arg(long, conflicts_with = "append_only")]
        no_append_only: bool,
        /// Frontmatter field to draw the display title from.
        #[arg(long, value_name = "FIELD")]
        title_field: Option<String>,
        /// Frontmatter field carrying the note's date.
        #[arg(long, value_name = "FIELD")]
        date_field: Option<String>,
    },

    /// Remove a note type. Idempotent: removing an absent type succeeds.
    Remove {
        /// Name of the note type to remove.
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum FieldCommands {
    /// Declare a schema field, or change one in place. Merges like
    /// `note-type set`: an omitted flag keeps its current value.
    Set {
        /// Note type the field belongs to.
        #[arg(long)]
        note_type: Option<String>,
        /// Field name.
        #[arg(long)]
        field: Option<String>,
        /// Scalar type: bool, int, float, string or date. Required when
        /// declaring; preserved when omitted on an edit.
        #[arg(long = "type", value_name = "TYPE")]
        ty: Option<String>,
        /// Default value, parsed according to the field's type. Pass an
        /// empty value to clear it.
        #[arg(long, value_name = "VALUE")]
        default: Option<String>,
        /// Require the field to be present.
        #[arg(long, conflicts_with = "no_required")]
        required: bool,
        /// Clear the required mark.
        #[arg(long, conflicts_with = "required")]
        no_required: bool,
        /// Comma-separated allowed values. Empty clears the list.
        #[arg(long, value_name = "VALUES")]
        values: Option<String>,
        /// Allow the field to be set through the setter surface.
        #[arg(long, conflicts_with = "no_settable")]
        settable: bool,
        /// Clear the settable flag.
        #[arg(long, conflicts_with = "settable")]
        no_settable: bool,
        /// Log a change to this field to the daily note.
        #[arg(long, conflicts_with = "no_log_on_change")]
        log_on_change: bool,
        /// Clear the log-on-change flag.
        #[arg(long, conflicts_with = "log_on_change")]
        no_log_on_change: bool,
    },

    /// Remove a schema field. Idempotent.
    Remove {
        /// Note type the field belongs to.
        #[arg(long)]
        note_type: Option<String>,
        /// Field name to remove.
        #[arg(long)]
        field: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlotCommands {
    /// Set how a declared tracking metric is plotted. `--plot none`
    /// keeps the metric collected and queryable but undrawn, which is
    /// what an absent `plot` key means.
    Set {
        /// Tracking activity, i.e. `[tracking.<activity>]`.
        #[arg(long)]
        activity: Option<String>,
        /// Metric name under that activity.
        #[arg(long)]
        metric: Option<String>,
        /// One of: none, line, column, area, scatter.
        #[arg(long)]
        plot: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum VarCommands {
    /// Set a static template variable, available to every template.
    Set {
        /// Variable name.
        #[arg(long)]
        name: Option<String>,
        /// Value the placeholder renders to.
        #[arg(long)]
        value: Option<String>,
    },

    /// Remove a static template variable. Idempotent.
    Remove {
        /// Variable name to remove.
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum PromptCommands {
    /// Set a prompted template variable — one the create path asks for
    /// interactively when it is unresolved.
    Set {
        /// Variable name.
        #[arg(long)]
        name: Option<String>,
        /// The question put to the user when it is unresolved.
        #[arg(long)]
        message: Option<String>,
    },

    /// Remove a prompted template variable. Idempotent.
    Remove {
        /// Variable name to remove.
        #[arg(long)]
        name: Option<String>,
    },
}

pub fn run(root: &Path, command: ConfigCommands, json: bool, no_interactive: bool) -> Result<()> {
    // These verbs deliberately never open the vault (see the module docs),
    // and that left them unable to tell a broken vault from no vault at
    // all: `config validate` answered "Config is valid." on an empty
    // directory, and `config var set` created a stray `.cuaderno/config.toml`,
    // half-initialising somewhere that was never a vault. Skipping
    // `Vault::new` is the point; skipping the directory check was an
    // oversight. Mirrors `reindex`, which checks the same way before it
    // deletes anything.
    let cuaderno_dir = root.join(cdno_core::paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        bail!(
            "no Cuaderno vault at {}; run `cdno init` to create one.",
            root.display()
        );
    }
    let interactive = crate::prompt::reports_interactively(no_interactive, json);
    match command {
        ConfigCommands::Show => show(root, json),
        ConfigCommands::Validate { file } => validate(root, file.as_deref(), json),
        ConfigCommands::Edit { editor } => edit(root, editor.as_deref(), interactive),
        ConfigCommands::NoteType { subcommand } => note_type(root, subcommand, json, interactive),
        ConfigCommands::Field { subcommand } => field(root, subcommand, json, interactive),
        ConfigCommands::Plot { subcommand } => plot(root, subcommand, json, interactive),
        ConfigCommands::Var { subcommand } => var(root, subcommand, json, interactive),
        ConfigCommands::Prompt { subcommand } => prompt_var(root, subcommand, json, interactive),
    }
}

/// Read the vault's config without opening the vault. The seam `show` and
/// the tests share, so "verbatim" is asserted on data rather than stdout.
pub fn read_raw(root: &Path) -> Result<cdno_domain::ConfigDocument> {
    let store = FsVaultStore::new(root);
    read_config_from(&store).context("reading .cuaderno/config.toml")
}

fn show(root: &Path, json: bool) -> Result<()> {
    let doc = read_raw(root)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "content": doc.content,
                "hash": doc.hash,
            }))?
        );
    } else {
        // Verbatim, and via `print!` rather than `println!`: the file's own
        // trailing newline is part of what "verbatim" means, and adding a
        // second one would break `cdno config show > config.toml`.
        print!("{}", doc.content);
    }
    Ok(())
}

fn validate(root: &Path, file: Option<&Path>, json: bool) -> Result<()> {
    let content = match file {
        Some(path) => {
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
        }
        None => {
            let store = FsVaultStore::new(root);
            read_config_from(&store)
                .context("reading .cuaderno/config.toml")?
                .content
        }
    };

    match validate_config_str(&content) {
        Ok(()) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({ "valid": true }))?
                );
            } else {
                println!("Config is valid.");
            }
            Ok(())
        }
        Err(err) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "valid": false,
                        "message": err.message,
                        "line": err.line,
                        "col": err.col,
                    }))?
                );
                // `--json` still has to fail the process: a script checking
                // the exit code must not read a rejection as a pass.
                std::process::exit(1);
            }
            // The message already names the line and column for a parse
            // error, and reads as prose for a validation one, so it needs
            // no further decoration.
            bail!("{}", err.message)
        }
    }
}

fn edit(root: &Path, editor_flag: Option<&str>, interactive: bool) -> Result<()> {
    // An editor needs a human. Failing fast is better than launching one
    // into a pipe, and matches how every other interactive path behaves.
    //
    // The test is the full interactivity rule, NOT the `--no-interactive`
    // flag alone: `is_interactive` requires stdin AND stdout to be
    // terminals, and stdin is the term that matters here. Gating on the
    // flag meant `cdno config edit < /dev/null` from a script spawned
    // `$EDITOR` against a closed stdin — which is precisely the regression
    // `docs/cli-ergonomics.md` warns not to simplify the formula into.
    if !interactive {
        bail!(
            "`cdno config edit` needs an interactive terminal. \
             Use `cdno config show` to read the config, or edit \
             .cuaderno/config.toml directly and check it with `cdno config validate`."
        );
    }

    let store = FsVaultStore::new(root);
    let original = read_config_from(&store).context("reading .cuaderno/config.toml")?;

    // Named `config.toml` inside a scratch directory so the editor sees a
    // `.toml` extension and highlights accordingly.
    let scratch = tempfile::tempdir().context("creating a scratch directory for the edit")?;
    let buffer = scratch.path().join("config.toml");
    std::fs::write(&buffer, &original.content)
        .with_context(|| format!("seeding {}", buffer.display()))?;

    let editor = crate::editor::resolve(editor_flag, &|key| std::env::var(key).ok(), None)?;

    match editor.spawn(&buffer)? {
        Some(0) => {}
        Some(code) => {
            // Mirrors `cdno open`: a non-zero editor exit is how a human
            // says "forget it", and `git commit` treats it the same way.
            eprintln!("Editor exited with status {code} — config unchanged.");
            std::process::exit(code);
        }
        None => {
            // The buffer is in a tempdir that is about to be removed, and
            // nothing has been written, so there is nothing to preserve.
            bail!(
                "that editor detached instead of waiting, so the edited config \
                 cannot be read back. Use an editor that waits — `code -w`, \
                 `vim`, `nano` — via --editor or $EDITOR."
            );
        }
    }

    let edited = std::fs::read_to_string(&buffer)
        .with_context(|| format!("reading back {}", buffer.display()))?;

    match finish_edit(&store, &original, &edited) {
        Ok(EditOutcome::Unchanged) => {
            println!("No changes.");
            Ok(())
        }
        Ok(EditOutcome::Saved { bytes }) => {
            println!("Config saved ({bytes} bytes).");
            Ok(())
        }
        // Both failure modes below wrote nothing, so the edit exists only in
        // the scratch buffer. Persisting it and naming the path is the whole
        // point — a rejected save must not also destroy the work.
        Err(ConfigSaveError::Validation(err)) => {
            let kept = preserve(scratch, &buffer, &edited);
            bail!(
                "that config would not open, so nothing was written:\n  {}\n\n\
                 Your edit is kept at {}",
                err.message,
                kept.display()
            )
        }
        Err(ConfigSaveError::Conflict) => {
            let kept = preserve(scratch, &buffer, &edited);
            bail!(
                "the config changed on disk while you were editing, so nothing \
                 was written rather than clobber it.\n\n\
                 Your edit is kept at {}\n\
                 Re-run `cdno config edit` and reapply it against the current file.",
                kept.display()
            )
        }
        Err(ConfigSaveError::Internal(message)) => {
            let kept = preserve(scratch, &buffer, &edited);
            bail!(
                "could not save the config: {message}\n\nYour edit is kept at {}",
                kept.display()
            )
        }
    }
}

/// Copy a rejected buffer somewhere it will outlive the scratch directory.
///
/// TAKES THE SCRATCH GUARD BY VALUE, and that is the point. `scratch` is a
/// `TempDir`, so the directory is removed when it drops — which happens on
/// the very `bail!` that tells the user where their edit is. The earlier
/// version returned the in-scratch path as its fallback and claimed it
/// "survives until the process exits"; both were wrong, so a user whose
/// save was refused AND whose temp copy failed was pointed at a path that
/// no longer existed by the time they read the message.
///
/// Now the fallback calls `TempDir::keep`, which disarms the deletion, so
/// the named path is really there. A rejected edit is the user's work: the
/// recovery path must not be the thing that loses it.
///
/// The happy path still copies out to a `tempfile`-created file — `O_EXCL`
/// and a random name, so a predictable path in a shared /tmp can neither
/// be pre-empted by a symlink nor clobber an earlier rejected edit — and
/// lets the scratch directory drop as usual.
pub fn preserve(scratch: tempfile::TempDir, buffer: &Path, edited: &str) -> std::path::PathBuf {
    let built = tempfile::Builder::new()
        .prefix("cdno-config-rejected-")
        .suffix(".toml")
        .tempfile();
    if let Ok(file) = built
        && let Ok((mut handle, path)) = file.keep()
    {
        use std::io::Write;
        if handle.write_all(edited.as_bytes()).is_ok() {
            return path;
        }
    }
    // Could not copy it out, so keep the scratch directory instead of
    // deleting it, and name the buffer still sitting in it.
    let kept = scratch.keep();
    kept.join(
        buffer
            .file_name()
            .map(std::path::Path::new)
            .unwrap_or_else(|| std::path::Path::new("config.toml")),
    )
}

/// What a completed edit did, once the buffer came back from the editor.
///
/// Separated from the editor round trip so the gate's composition — the
/// no-op short-circuit and the three ways a save is refused — is testable
/// without spawning a program.
#[derive(Debug, PartialEq, Eq)]
pub enum EditOutcome {
    /// The buffer came back byte-identical, so nothing was written. Checked
    /// BEFORE the save: an unchanged buffer must not trip the
    /// compare-and-swap just because someone else touched the file, and it
    /// must not rewrite the config to an identical byte string either.
    Unchanged,
    /// The candidate validated, the compare-and-swap held, and the file was
    /// written. `bytes` is the persisted length, re-read from disk.
    Saved { bytes: usize },
}

/// Apply an edited buffer through the save gate.
///
/// The `Err` arm carries the domain's own [`ConfigSaveError`] untouched, so
/// the caller can phrase each refusal — and so a future MCP surface can
/// branch on the variant rather than parse prose.
pub fn finish_edit(
    store: &dyn cdno_core::store::VaultStore,
    original: &cdno_domain::ConfigDocument,
    edited: &str,
) -> Result<EditOutcome, ConfigSaveError> {
    if edited == original.content {
        return Ok(EditOutcome::Unchanged);
    }
    let saved = save_config_to(store, edited, &original.hash)?;
    Ok(EditOutcome::Saved {
        bytes: saved.content.len(),
    })
}

// ---------------------------------------------------------------------------
// The structured setters
// ---------------------------------------------------------------------------
//
// Every verb below is the same three steps: read the config, hand the buffer
// to one `cdno_core::config_edit` function, and push the candidate back
// through `finish_edit` — the SAME gate `cdno config edit` uses. So a
// structured edit that would produce a config the vault cannot open is
// refused exactly like a hand-edited one, and a concurrent change is still
// a conflict rather than a clobber. There is no second write path here.
//
// ## Why `set` merges rather than replaces
//
// `set_note_type` and `set_schema_field` REPLACE their whole table: they
// write every key the model carries and remove every key it does not. That
// is right for the desktop, whose form is pre-populated with the current
// values before it sends the struct back — the form always has the whole
// truth to re-send.
//
// A CLI flag set is not the whole truth. Passing the flags straight through
// would mean `cdno config note-type set --name people --folder people` on an
// existing type silently dropped its `required` list, its template and its
// date field, because those flags were absent. So these verbs read the
// current value first and apply only the flags actually given. An omitted
// flag keeps what is there; an empty value (`--template ''`) clears it.
// Creating a type is the one case with nothing to merge into, which is why
// `--folder` is required there and promptable, and optional afterwards.

/// Parse the config into its model so an edit can merge into what is
/// already there.
///
/// A config that does not deserialise cannot be merged into: there is
/// nothing to preserve and nothing to compare against, and guessing would
/// either drop the user's keys or invent them. The structured verbs refuse
/// on it and name the two verbs that do work on a broken config, both of
/// which deliberately avoid the model for exactly this reason.
fn read_model(content: &str) -> Result<VaultConfig> {
    toml::from_str(content).map_err(|err| {
        anyhow::anyhow!(
            "this config cannot be read, so a field cannot be changed in place:\n  {err}\n\n\
             `cdno config show` and `cdno config validate` still work on a broken \
             config — fix it with `cdno config edit`, then re-run."
        )
    })
}

/// Turn a save-gate refusal into the message for a structured edit.
///
/// Unlike `edit`, there is no editor buffer to preserve: the input was
/// flags, so re-running is the whole recovery. What matters is saying
/// plainly that nothing was written.
fn describe(err: ConfigSaveError) -> anyhow::Error {
    match err {
        ConfigSaveError::Validation(e) => anyhow::anyhow!(
            "that change would leave a config the vault cannot open, so nothing was \
             written:\n  {}\n\nThe config on disk is unchanged.",
            e.message
        ),
        ConfigSaveError::Conflict => anyhow::anyhow!(
            "the config changed on disk while this edit was being prepared, so nothing \
             was written rather than overwrite it. Re-run to apply the change against \
             the current file."
        ),
        ConfigSaveError::Internal(message) => {
            anyhow::anyhow!("could not save the config: {message}")
        }
    }
}

/// Report what a structured edit did.
///
/// The `Unchanged` arm is load-bearing rather than cosmetic: setting a key
/// to the value it already holds writes nothing at all, so a re-run — or a
/// script that applies the same config repeatedly — does not churn the file
/// or burn the compare-and-swap.
fn emit(json: bool, outcome: &EditOutcome, saved: &str) -> Result<()> {
    match outcome {
        EditOutcome::Unchanged => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({ "changed": false }))?
                );
            } else {
                println!("No change — the config already says that.");
            }
        }
        EditOutcome::Saved { bytes } => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({ "changed": true, "bytes": bytes })
                    )?
                );
            } else {
                println!("{saved}");
            }
        }
    }
    Ok(())
}

/// Read, transform, save. The shared spine of every structured verb.
fn apply(
    root: &Path,
    transform: impl FnOnce(&str) -> std::result::Result<String, ConfigEditError>,
) -> Result<EditOutcome> {
    let store = FsVaultStore::new(root);
    let original = read_config_from(&store).context("reading .cuaderno/config.toml")?;
    let candidate = transform(&original.content)?;
    finish_edit(&store, &original, &candidate).map_err(describe)
}

/// Merge a comma-separated list flag: absent keeps, empty clears.
pub fn merge_list(flag: Option<String>, current: &[String]) -> Vec<String> {
    match flag {
        None => current.to_vec(),
        Some(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

/// Merge an optional scalar flag: absent keeps, empty clears.
pub fn merge_opt(flag: Option<String>, current: Option<&String>) -> Option<String> {
    match flag {
        None => current.cloned(),
        Some(raw) if raw.is_empty() => None,
        Some(raw) => Some(raw),
    }
}

/// Merge a paired `--x` / `--no-x` flag: neither given keeps the current
/// value. Clap's `conflicts_with` makes both-at-once unreachable.
pub fn merge_flag(on: bool, off: bool, current: bool) -> bool {
    if on {
        true
    } else if off {
        false
    } else {
        current
    }
}

/// The tri-state form, for the `Option<bool>` keys where absent and
/// explicit-false mean the same thing to the writer but the current value
/// still has to survive an unrelated edit.
pub fn merge_tri(on: bool, off: bool, current: Option<bool>) -> Option<bool> {
    if on {
        Some(true)
    } else if off {
        None
    } else {
        current
    }
}

pub fn parse_field_type(raw: &str) -> Result<FieldType> {
    match raw.to_ascii_lowercase().as_str() {
        "bool" => Ok(FieldType::Bool),
        "int" => Ok(FieldType::Int),
        "float" => Ok(FieldType::Float),
        "string" => Ok(FieldType::String),
        "date" => Ok(FieldType::Date),
        other => bail!("unknown field type '{other}' — valid: bool, int, float, string, date"),
    }
}

pub fn parse_plot_kind(raw: &str) -> Result<PlotKind> {
    match raw.to_ascii_lowercase().as_str() {
        "none" => Ok(PlotKind::None),
        "line" => Ok(PlotKind::Line),
        "column" => Ok(PlotKind::Column),
        "area" => Ok(PlotKind::Area),
        "scatter" => Ok(PlotKind::Scatter),
        other => bail!("unknown plot kind '{other}' — valid: none, line, column, area, scatter"),
    }
}

/// Parse a `--default` against the field's own type.
///
/// Typed here rather than left to the save gate so the error names the flag
/// and the expected form. A `date` default is authored as a quoted
/// `YYYY-MM-DD` string, which is the shape `config_edit`'s own writer emits,
/// so it is parsed for validity and stored as a string.
pub fn parse_default(raw: &str, ty: FieldType) -> Result<toml::Value> {
    match ty {
        FieldType::Bool => raw
            .parse::<bool>()
            .map(toml::Value::Boolean)
            .map_err(|_| anyhow::anyhow!("--default '{raw}' is not a bool — use true or false")),
        FieldType::Int => raw
            .parse::<i64>()
            .map(toml::Value::Integer)
            .map_err(|_| anyhow::anyhow!("--default '{raw}' is not an integer")),
        FieldType::Float => raw
            .parse::<f64>()
            .map(toml::Value::Float)
            .map_err(|_| anyhow::anyhow!("--default '{raw}' is not a number")),
        FieldType::String => Ok(toml::Value::String(raw.to_owned())),
        FieldType::Date => match chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
            Ok(_) => Ok(toml::Value::String(raw.to_owned())),
            Err(_) => bail!("--default '{raw}' is not a date — use YYYY-MM-DD"),
        },
    }
}

/// Confirm a prompted edit, per the flags-and-prompts convention: only
/// when something was actually asked for interactively.
fn confirm_if_prompted(prompted: bool, preview: &str) -> Result<bool> {
    if !prompted {
        return Ok(true);
    }
    crate::prompt::confirm_preview(preview)
}

fn note_type(root: &Path, command: NoteTypeCommands, json: bool, interactive: bool) -> Result<()> {
    match command {
        NoteTypeCommands::Set {
            name,
            folder,
            required,
            optional,
            template,
            append_only,
            no_append_only,
            title_field,
            date_field,
        } => {
            let mut prompted = false;
            let name = gather_or_error(name, "name", interactive, &mut prompted, || {
                prompt_text("Name of the note type")
            })?;
            let store = FsVaultStore::new(root);
            let original = read_config_from(&store).context("reading .cuaderno/config.toml")?;
            let model = read_model(&original.content)?;
            let current = model.note_types.get(&name);

            // The only conditionally-required input: a new type must say
            // where its notes live, an existing one already has.
            let folder = match current {
                Some(existing) => folder.unwrap_or_else(|| existing.folder.clone()),
                None => gather_or_error(folder, "folder", interactive, &mut prompted, || {
                    prompt_text(&format!("Folder for the new note type '{name}'"))
                })?,
            };

            let note_type = CustomNoteType {
                folder,
                required: merge_list(required, current.map_or(&[], |c| c.required.as_slice())),
                optional: merge_list(optional, current.map_or(&[], |c| c.optional.as_slice())),
                template: merge_opt(template, current.and_then(|c| c.template.as_ref())),
                append_only: merge_flag(
                    append_only,
                    no_append_only,
                    current.is_some_and(|c| c.append_only),
                ),
                title_field: merge_opt(title_field, current.and_then(|c| c.title_field.as_ref())),
                date_field: merge_opt(date_field, current.and_then(|c| c.date_field.as_ref())),
            };

            let verb = if current.is_some() {
                "Update"
            } else {
                "Create"
            };
            if !confirm_if_prompted(
                prompted,
                &format!(
                    "{verb} note type '{name}' with folder '{}'.",
                    note_type.folder
                ),
            )? {
                println!("Cancelled — nothing was written.");
                return Ok(());
            }

            let candidate = config_edit::set_note_type(&original.content, &name, &note_type)?;
            let outcome = finish_edit(&store, &original, &candidate).map_err(describe)?;
            emit(json, &outcome, &format!("Note type '{name}' saved."))
        }

        NoteTypeCommands::Remove { name } => {
            let mut prompted = false;
            let name = gather_or_error(name, "name", interactive, &mut prompted, || {
                prompt_text("Name of the note type to remove")
            })?;
            let outcome = apply(root, |content| {
                config_edit::remove_note_type(content, &name)
            })?;
            // Idempotent at the domain layer, so an absent type reports
            // "no change" rather than an error — a stale delete or a
            // re-run is a success, which is what makes this scriptable.
            emit(json, &outcome, &format!("Note type '{name}' removed."))
        }
    }
}

fn field(root: &Path, command: FieldCommands, json: bool, interactive: bool) -> Result<()> {
    match command {
        FieldCommands::Set {
            note_type,
            field,
            ty,
            default,
            required,
            no_required,
            values,
            settable,
            no_settable,
            log_on_change,
            no_log_on_change,
        } => {
            let mut prompted = false;
            let note_type =
                gather_or_error(note_type, "note-type", interactive, &mut prompted, || {
                    prompt_text("Note type the field belongs to")
                })?;
            let field = gather_or_error(field, "field", interactive, &mut prompted, || {
                prompt_text("Field name")
            })?;
            let store = FsVaultStore::new(root);
            let original = read_config_from(&store).context("reading .cuaderno/config.toml")?;
            let model = read_model(&original.content)?;
            // The same bug class `plot set` guards against, one verb over:
            // `validate_reserved_schema_fields` deliberately skips schema
            // names it does not know, so a typo in `--note-type` wrote
            // `[schemas.porject.fields.status]`, reported success, and
            // validated clean — a phantom schema attached to nothing.
            let known = model
                .note_types
                .keys()
                .cloned()
                .chain(
                    cdno_domain::note_type::NoteType::ALL
                        .iter()
                        .map(|t| t.as_str().to_owned()),
                )
                .collect::<Vec<_>>();
            if !known.iter().any(|name| name == &note_type) {
                let mut sorted = known;
                sorted.sort();
                bail!("{}", unknown_name("note type", &note_type, &sorted));
            }

            let current = model
                .schemas
                .get(&note_type)
                .and_then(|schema| schema.fields.get(&field));

            // A field's type is what every other key is interpreted
            // against, so it is required when declaring one and preserved
            // afterwards — the same shape as a note type's folder.
            let ty = match current {
                Some(existing) => match ty {
                    Some(raw) => parse_field_type(&raw)?,
                    None => existing.ty,
                },
                None => {
                    let raw = gather_or_error(ty, "type", interactive, &mut prompted, || {
                        prompt_text(&format!(
                            "Type for '{note_type}.{field}' (bool, int, float, string, date)"
                        ))
                    })?;
                    parse_field_type(&raw)?
                }
            };

            // Parsed against the type resolved above, so `--default` on an
            // existing field is checked against the type it actually has
            // rather than a guess.
            let default = match default {
                None => current.and_then(|c| c.default.clone()),
                Some(raw) if raw.is_empty() => None,
                Some(raw) => Some(parse_default(&raw, ty)?),
            };

            let spec = FieldSpec {
                ty,
                default,
                required: merge_flag(required, no_required, current.is_some_and(|c| c.required)),
                values: match values {
                    None => current.and_then(|c| c.values.clone()),
                    Some(raw) => {
                        let list = merge_list(Some(raw), &[]);
                        if list.is_empty() { None } else { Some(list) }
                    }
                },
                // Deliberately carried through untouched. `set_schema_field`
                // does not write `list` at all — it is unimplemented, and a
                // hand-authored value must survive an unrelated edit — so
                // this mirrors the writer rather than inventing a flag for
                // a key nothing reads yet.
                list: current.and_then(|c| c.list),
                settable: merge_tri(settable, no_settable, current.and_then(|c| c.settable)),
                log_on_change: merge_tri(
                    log_on_change,
                    no_log_on_change,
                    current.and_then(|c| c.log_on_change),
                ),
            };

            if !confirm_if_prompted(
                prompted,
                &format!(
                    "Declare '{note_type}.{field}' as type {}.",
                    spec.ty.as_str()
                ),
            )? {
                println!("Cancelled — nothing was written.");
                return Ok(());
            }

            let candidate =
                config_edit::set_schema_field(&original.content, &note_type, &field, &spec)?;
            let outcome = finish_edit(&store, &original, &candidate).map_err(describe)?;
            emit(
                json,
                &outcome,
                &format!("Field '{note_type}.{field}' saved."),
            )
        }

        FieldCommands::Remove { note_type, field } => {
            let mut prompted = false;
            let note_type =
                gather_or_error(note_type, "note-type", interactive, &mut prompted, || {
                    prompt_text("Note type the field belongs to")
                })?;
            let field = gather_or_error(field, "field", interactive, &mut prompted, || {
                prompt_text("Field name to remove")
            })?;
            let outcome = apply(root, |content| {
                config_edit::remove_schema_field(content, &note_type, &field)
            })?;
            emit(
                json,
                &outcome,
                &format!("Field '{note_type}.{field}' removed."),
            )
        }
    }
}

fn plot(root: &Path, command: PlotCommands, json: bool, interactive: bool) -> Result<()> {
    let PlotCommands::Set {
        activity,
        metric,
        plot,
    } = command;
    let mut prompted = false;
    let activity = gather_or_error(activity, "activity", interactive, &mut prompted, || {
        prompt_text("Tracking activity")
    })?;
    let metric = gather_or_error(metric, "metric", interactive, &mut prompted, || {
        prompt_text("Metric name under that activity")
    })?;

    // `set_metric_plot` writes into `[tracking.<activity>.metrics.<metric>]`,
    // creating every table on the way down. That is right for the desktop,
    // whose form only ever offers metrics that exist — but it means a typed
    // `--metric` typo here would be WRITTEN rather than refused: a phantom
    // metric declared on an activity, reported as a success, with the real
    // metric's plot left untouched. The config still validates, so nothing
    // downstream catches it either.
    //
    // `set_metric_plot`'s own docs state the "already declared" precondition.
    // This CLI is the first caller able to break it, so the check belongs
    // here.
    let store = FsVaultStore::new(root);
    let original = read_config_from(&store).context("reading .cuaderno/config.toml")?;
    let model = read_model(&original.content)?;
    let spec = model.tracking.get(&activity).ok_or_else(|| {
        let known = model.tracking.keys().cloned().collect::<Vec<_>>();
        anyhow::anyhow!("{}", unknown_name("tracking activity", &activity, &known))
    })?;
    if !spec.metrics.contains_key(&metric) {
        let known = spec.metrics.keys().cloned().collect::<Vec<_>>();
        bail!(
            "{}",
            unknown_name(
                &format!("metric on tracking activity '{activity}'"),
                &metric,
                &known
            )
        );
    }

    let raw = gather_or_error(plot, "plot", interactive, &mut prompted, || {
        prompt_text(&format!(
            "Plot for '{activity}.{metric}' (none, line, column, area, scatter)"
        ))
    })?;
    let kind = parse_plot_kind(&raw)?;

    if !confirm_if_prompted(prompted, &format!("Plot '{activity}.{metric}' as {raw}."))? {
        println!("Cancelled — nothing was written.");
        return Ok(());
    }

    // Deliberately NOT through `apply`, which would take a second read.
    // The activity and metric were resolved against `original` above; a
    // fresh read here would make the compare-and-swap baseline newer than
    // the state that was checked, so a metric deleted while the prompt was
    // open would be recreated as a phantom, and a concurrent edit would be
    // overwritten instead of reported as a conflict. `note-type set` and
    // `field set` already hold their pre-prompt read for the same reason.
    let candidate = config_edit::set_metric_plot(&original.content, &activity, &metric, kind)?;
    let outcome = finish_edit(&store, &original, &candidate).map_err(describe)?;
    emit(
        json,
        &outcome,
        &format!("Plot for '{activity}.{metric}' set to {raw}."),
    )
}

fn var(root: &Path, command: VarCommands, json: bool, interactive: bool) -> Result<()> {
    match command {
        VarCommands::Set { name, value } => {
            let mut prompted = false;
            let name = gather_or_error(name, "name", interactive, &mut prompted, || {
                prompt_text("Variable name")
            })?;
            let value = gather_or_error(value, "value", interactive, &mut prompted, || {
                prompt_text(&format!("Value for {{{{{name}}}}}"))
            })?;
            if !confirm_if_prompted(prompted, &format!("Set {{{{{name}}}}} to '{value}'."))? {
                println!("Cancelled — nothing was written.");
                return Ok(());
            }
            let outcome = apply(root, |content| {
                config_edit::set_variable(content, &name, &value)
            })?;
            emit(json, &outcome, &format!("Variable '{name}' saved."))
        }
        VarCommands::Remove { name } => {
            let mut prompted = false;
            let name = gather_or_error(name, "name", interactive, &mut prompted, || {
                prompt_text("Variable name to remove")
            })?;
            let outcome = apply(root, |content| config_edit::remove_variable(content, &name))?;
            emit(json, &outcome, &format!("Variable '{name}' removed."))
        }
    }
}

fn prompt_var(root: &Path, command: PromptCommands, json: bool, interactive: bool) -> Result<()> {
    match command {
        PromptCommands::Set { name, message } => {
            let mut prompted = false;
            let name = gather_or_error(name, "name", interactive, &mut prompted, || {
                prompt_text("Variable name")
            })?;
            let message = gather_or_error(message, "message", interactive, &mut prompted, || {
                prompt_text(&format!("Question to ask for {{{{{name}}}}}"))
            })?;
            if !confirm_if_prompted(prompted, &format!("Ask '{message}' for {{{{{name}}}}}."))? {
                println!("Cancelled — nothing was written.");
                return Ok(());
            }
            let outcome = apply(root, |content| {
                config_edit::set_prompt_variable(content, &name, &message)
            })?;
            emit(
                json,
                &outcome,
                &format!("Prompted variable '{name}' saved."),
            )
        }
        PromptCommands::Remove { name } => {
            let mut prompted = false;
            let name = gather_or_error(name, "name", interactive, &mut prompted, || {
                prompt_text("Prompted variable name to remove")
            })?;
            let outcome = apply(root, |content| {
                config_edit::remove_prompt_variable(content, &name)
            })?;
            emit(
                json,
                &outcome,
                &format!("Prompted variable '{name}' removed."),
            )
        }
    }
}

/// Phrase an unknown-name refusal, listing what the config does declare.
///
/// The valid set is the whole point: these names are free text on the
/// command line, so the likely cause of a miss is a typo, and the fix is
/// usually visible the moment the real names are printed.
fn unknown_name(what: &str, given: &str, known: &[String]) -> String {
    if known.is_empty() {
        format!("no {what} is declared in .cuaderno/config.toml, so '{given}' cannot be set")
    } else {
        format!("unknown {what} '{given}' — declared: {}", known.join(", "))
    }
}
