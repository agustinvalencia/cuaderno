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

use cdno_core::store::FsVaultStore;
use cdno_domain::vault::config::{read_config_from, save_config_to};
use cdno_domain::{ConfigSaveError, validate_config_str};

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
}

pub fn run(root: &Path, command: ConfigCommands, json: bool, no_interactive: bool) -> Result<()> {
    match command {
        ConfigCommands::Show => show(root, json),
        ConfigCommands::Validate { file } => validate(root, file.as_deref(), json),
        ConfigCommands::Edit { editor } => edit(root, editor.as_deref(), no_interactive),
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

fn edit(root: &Path, editor_flag: Option<&str>, no_interactive: bool) -> Result<()> {
    // An editor needs a human. Failing fast is better than launching one
    // into a pipe, and matches how every other interactive path behaves.
    if no_interactive {
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
            let kept = preserve(&buffer, &original.content, &edited);
            bail!(
                "that config would not open, so nothing was written:\n  {}\n\n\
                 Your edit is kept at {}",
                err.message,
                kept.display()
            )
        }
        Err(ConfigSaveError::Conflict) => {
            let kept = preserve(&buffer, &original.content, &edited);
            bail!(
                "the config changed on disk while you were editing, so nothing \
                 was written rather than clobber it.\n\n\
                 Your edit is kept at {}\n\
                 Re-run `cdno config edit` and reapply it against the current file.",
                kept.display()
            )
        }
        Err(ConfigSaveError::Internal(message)) => {
            let kept = preserve(&buffer, &original.content, &edited);
            bail!(
                "could not save the config: {message}\n\nYour edit is kept at {}",
                kept.display()
            )
        }
    }
}

/// Copy a rejected buffer somewhere it will outlive the scratch directory.
///
/// Best-effort by design: this runs on a path that is already failing, and
/// an error here must not replace the real reason the save was refused. If
/// the copy cannot be made the scratch path is still named — it survives
/// until the process exits, which is long enough to retrieve by hand.
fn preserve(buffer: &Path, _original: &str, edited: &str) -> std::path::PathBuf {
    let target =
        std::env::temp_dir().join(format!("cdno-config-rejected-{}.toml", std::process::id()));
    match std::fs::write(&target, edited) {
        Ok(()) => target,
        Err(_) => buffer.to_path_buf(),
    }
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
