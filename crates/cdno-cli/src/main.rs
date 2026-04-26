//! `cdno` — command-line interface for Cuaderno vaults.
//!
//! Thin shim: parses arguments, resolves the path arg (None → CWD),
//! and delegates to a library handler. All real work lives in
//! [`cdno_cli`].

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use cdno_cli::commands;

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
        Commands::Init { path } => {
            let target = match path {
                Some(p) => p,
                None => std::env::current_dir()
                    .context("could not determine the current working directory")?,
            };
            commands::init::run(&target)
        }
    }
}
