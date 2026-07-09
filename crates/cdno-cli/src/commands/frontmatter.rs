//! `cdno frontmatter` subcommands: the generic, schema-driven frontmatter
//! setter (#301).
//!
//! `frontmatter set <note> <key> <value>` writes a single typed frontmatter
//! field declared under `[schemas.<type>.fields.<key>]` — the tooling-safe way
//! to toggle a daily flag (`meds`/`workout`/`closed`) or any other declared,
//! settable field without a hand-edit that would desync the index.

use std::path::Path;

use anyhow::Result;
use chrono::Local;
use clap::Subcommand;

use crate::bootstrap;

#[derive(Debug, Subcommand)]
pub enum FrontmatterCommands {
    /// Set a declared, settable frontmatter field on a note. `note` is
    /// `today`, a `YYYY-MM-DD` date (both resolve to the daily note), or a
    /// vault-relative note path. `key` must be declared `settable = true`
    /// under `[schemas.<type>.fields.<key>]`; `value` is coerced to the
    /// field's declared type.
    Set {
        /// The note to edit: `today`, a `YYYY-MM-DD` date, or a
        /// vault-relative path (e.g. `projects/foo.md`).
        note: String,
        /// The frontmatter field name to set.
        key: String,
        /// The new value, as a string (coerced to the declared type).
        value: String,
    },
}

pub fn run(root: &Path, command: FrontmatterCommands, json: bool) -> Result<()> {
    match command {
        FrontmatterCommands::Set { note, key, value } => {
            let (vault, _report) = bootstrap::open_vault(root)?;
            // Report the primary path; the outcome's full touched-path set is
            // for the desktop echo journal and is not needed here.
            let outcome = vault.set_frontmatter(Local::now().naive_local(), &note, &key, &value)?;
            let path = outcome.primary;
            let message = if outcome.paths.is_empty() {
                format!("No change: {key} already set on {path}")
            } else {
                format!("Set {key} on {path}")
            };
            crate::output::emit_write_result(json, &path.to_string(), &message)
        }
    }
}
