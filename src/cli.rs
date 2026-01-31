//! CLI argument definitions.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Output format for CLI responses.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable output.
    Plain,
    /// Machine-readable JSON output.
    Json,
}

/// Sort order for entry listing.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SortOrder {
    /// Order by date (desc).
    #[value(alias = "date_desc")]
    DateDesc,
    /// Order by date (asc).
    #[value(alias = "date_asc")]
    DateAsc,
    /// Order by first seen (desc).
    #[value(alias = "first_seen_desc")]
    FirstSeenDesc,
    /// Order by first seen (asc).
    #[value(alias = "first_seen_asc")]
    FirstSeenAsc,
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

    /// List entry summaries.
    List {
        /// Query string for tag filters.
        #[arg(long)]
        query: Option<String>,
        /// Sort order.
        #[arg(long, value_enum)]
        sort: Option<SortOrder>,
        /// Number of items to return.
        #[arg(long)]
        limit: Option<usize>,
        /// Pagination cursor.
        #[arg(long)]
        cursor: Option<String>,
    },

    /// View entry detail by id.
    View {
        /// Entry id.
        id: i64,
    },

    /// Update entry tags.
    Mark {
        /// Mark operation to perform.
        #[command(subcommand)]
        command: MarkCommand,
    },
}

/// Mark operation subcommands.
#[derive(Debug, Subcommand)]
pub enum MarkCommand {
    /// Mark entries as read (remove unread tag).
    Read { ids: Vec<i64> },
    /// Mark entries as unread (add unread tag).
    Unread { ids: Vec<i64> },
    /// Mark entries as starred (add star tag).
    Star { ids: Vec<i64> },
    /// Mark entries as unstarred (remove star tag).
    Unstar { ids: Vec<i64> },
    /// Add/remove custom tags.
    Tag {
        /// Entry ids.
        ids: Vec<i64>,
        /// Tags to add (comma-separated).
        #[arg(long)]
        add: Option<String>,
        /// Tags to remove (comma-separated).
        #[arg(long)]
        remove: Option<String>,
    },
}
