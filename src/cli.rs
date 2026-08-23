//! CLI argument definitions.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

const LIST_QUERY_LONG_HELP: &str = "\
Query string for entry filters

Supported terms:
  unread
  tag:<expr>         tag expression: AND/OR/NOT, &, |, !, ()
  -tag:<expr>        exclude tags
  <term>             title search term
  -<term>            exclude title search term
  (<expr>)           title term expression group
  -(<expr>)          exclude title term expression group
  feed:<id>|\"<title>\"
  after:<YYYY-MM-DD|Nd|Nw|Nm|Ny>
  before:<YYYY-MM-DD|Nd|Nw|Nm|Ny>

Examples:
  --query 'after:1w'
  --query 'tag:( rust & cli )'
  --query 'tag:rust -tag:(archived|misc)'
  --query 'foo -bar'
  --query '(color|colour) -(draft|sponsored)'";

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

impl SortOrder {
    /// Returns the canonical string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            SortOrder::DateDesc => "date_desc",
            SortOrder::DateAsc => "date_asc",
            SortOrder::FirstSeenDesc => "first_seen_desc",
            SortOrder::FirstSeenAsc => "first_seen_asc",
        }
    }
}

/// Picofeedr CLI arguments.
#[derive(Debug, Parser)]
#[command(name = "picofeedr", version, about = "Local-first feed reader backend")]
pub struct Cli {
    /// Path to config.toml.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Override storage root directory (contains db.sqlite and data/).
    #[arg(long)]
    pub storage_root: Option<PathBuf>,

    /// Output format for CLI responses.
    #[arg(short = 'o', long, value_enum)]
    pub output: Option<OutputFormat>,

    /// Enable debug diagnostics on stderr.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub debug: bool,

    /// CLI command to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print version information.
    Version,

    /// List tags stored in the database.
    Tags,

    /// Show lightweight database status metadata.
    Status,

    /// Show feed state recorded in the local database.
    Feeds {
        /// Append feed id as the last column in plain output.
        #[arg(short = 'i', long, action = clap::ArgAction::SetTrue)]
        id: bool,
    },

    /// Sync feeds and ingest new entries.
    Sync {
        /// Validate feeds YAML config without running.
        #[arg(long, action = clap::ArgAction::SetTrue)]
        check: bool,
    },

    /// List entry summaries.
    List {
        /// Query string for filters and title word search.
        #[arg(short = 'q', long, long_help = LIST_QUERY_LONG_HELP, allow_hyphen_values = true)]
        query: Option<String>,
        /// Sort order.
        #[arg(short = 's', long, value_enum)]
        sort: Option<SortOrder>,
        /// Number of items to return.
        #[arg(short = 'l', long)]
        limit: Option<usize>,
        /// Pagination cursor.
        #[arg(long)]
        cursor: Option<String>,
        /// Append entry id as the last column in plain output.
        #[arg(short = 'i', long, action = clap::ArgAction::SetTrue)]
        id: bool,
    },

    /// View entry detail by id.
    View {
        /// Entry id.
        #[arg(allow_hyphen_values = true)]
        id: String,
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
    Read {
        /// Entry ids.
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        ids: Vec<String>,
    },
    /// Mark entries as unread (add unread tag).
    Unread {
        /// Entry ids.
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        ids: Vec<String>,
    },
    /// Add/remove custom tags.
    Tag {
        /// Entry ids.
        #[arg(required = true, num_args = 1..)]
        ids: Vec<String>,
        /// Tags to add (comma-separated).
        #[arg(short = 'a', long)]
        add: Option<String>,
        /// Tags to remove (comma-separated).
        #[arg(short = 'r', long)]
        remove: Option<String>,
    },
}
