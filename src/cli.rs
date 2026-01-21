//! CLI argument definitions.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Feeder CLI arguments.
#[derive(Debug, Parser)]
#[command(name = "feeder", version, about = "Local-first feed reader backend")]
pub struct Cli {
    /// Path to config.toml.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Override database path for testing.
    #[arg(long)]
    pub db: Option<PathBuf>,

    /// CLI command to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print a simple health response.
    Ping,

    /// Print version information.
    Version,

    /// List tags stored in the database.
    Tags,

    /// List feeds or compare config with the database.
    Feeds {
        /// Show config differences without updating the database.
        #[arg(long, action = clap::ArgAction::SetTrue)]
        config_check: bool,
    },

    /// Sync feeds and ingest new entries.
    Sync,
}
