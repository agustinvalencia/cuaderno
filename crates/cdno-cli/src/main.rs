//! `cdno` — command-line interface for Cuaderno vaults.
//!
//! `main` parses arguments and dispatches to a subcommand handler.
//! Commands that operate on an existing vault open it through the
//! bootstrap helper (FsVaultStore + SqliteIndex + reconciliation);
//! `init` is the exception, since it creates the vault rather than
//! opening one.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod bootstrap;
mod commands;

#[derive(Debug, Parser)]
#[command(
    name = "cdno",
    about = "Cuaderno: a Research Logbook Method vault manager",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create a new vault: folder tree, .cuaderno/ config, default templates.
    Init {
        /// Target directory. Defaults to the current working directory.
        path: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { path } => commands::init::run(path.as_deref()),
    }
}
