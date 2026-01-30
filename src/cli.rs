//! CLI argument definitions.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Output format for CLI responses.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// Machine-readable JSON output.
    Json,
    /// Human-readable output.
    Plain,
}

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

    /// Output format for CLI responses.
    #[arg(long, value_enum)]
    pub output: Option<OutputFormat>,

    /// Enable debug diagnostics on stderr.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub debug: bool,

    /// Enable verbose trace diagnostics on stderr.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub trace: bool,

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
