//! Picofeedr CLI entrypoint.

mod cli;
mod config;
mod content_ref;
mod db;
mod entry;
mod error;
mod feed;
mod identity;
mod query;
mod response;
mod sync;
mod tag;
mod time;

use crate::cli::{Cli, Command, MarkCommand, OutputFormat, SortOrder};
use crate::config::feeds::ConfigCheckReport;
use crate::entry::{EntryDetail, EntryListResponse};
use crate::error::AppError;
use crate::feed::FeedListResponse;
use crate::query::EntryQuery;
use crate::response::Envelope;
use crate::sync::SyncSummary;
use crate::tag::TagManager;
use clap::Parser;
use serde_json::json;
use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::process::ExitCode;
use tracing::{debug, trace};

/// Execution results for CLI commands.
enum CommandOutput {
    Ping,
    Version {
        api_version: &'static str,
        schema_version: i64,
        build: &'static str,
    },
    Tags {
        tags: Vec<String>,
    },
    FeedsList {
        feeds: FeedListResponse,
    },
    Sync {
        summary: SyncSummary,
    },
    List {
        list: EntryListResponse,
    },
    View {
        detail: EntryDetail,
    },
    Mark {
        updated: usize,
    },
}

/// Runs the CLI and prints JSON output or error to stdout.
fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => return handle_cli_parse_error(&args, error),
    };
    let output = resolve_effective_output(&cli);
    init_logging(resolve_effective_log_level(&cli));
    debug!(?output, ?cli.command, "resolved CLI output and command");
    if matches!(cli.command, Command::Feeds { config_check: true }) {
        match run_config_check(&cli, output) {
            Ok(exit_code) => return exit_code,
            Err(error) => {
                maybe_print_diagnostics(&cli, &error);
                match output {
                    OutputFormat::Json => {
                        print_json_or_fallback(&Envelope::<serde_json::Value>::fatal(&error))
                    }
                    OutputFormat::Plain => eprintln!("{error}"),
                }
                return ExitCode::from(1);
            }
        }
    }
    if let Err(error) = run(&cli, output) {
        maybe_print_diagnostics(&cli, &error);
        match output {
            OutputFormat::Json => {
                print_json_or_fallback(&Envelope::<serde_json::Value>::fatal(&error))
            }
            OutputFormat::Plain => eprintln!("{error}"),
        }
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Executes the CLI command and prints a JSON response.
fn run(cli: &Cli, output: OutputFormat) -> Result<(), AppError> {
    let result = execute_command(cli)?;
    match output {
        OutputFormat::Json => render_json(&result)?,
        OutputFormat::Plain => render_plain(&result),
    }
    Ok(())
}

/// Runs static feeds config validation without touching the database.
fn run_config_check(cli: &Cli, output: OutputFormat) -> Result<ExitCode, AppError> {
    let config = load_config(cli)?;
    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
    let report = feeds_config.validate();
    match output {
        OutputFormat::Json => {
            print_json_or_fallback(&Envelope::ok(serde_json::to_value(&report)?));
        }
        OutputFormat::Plain => render_config_check_plain(&report),
    }
    Ok(if report.valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// Executes the CLI command and returns the result.
fn execute_command(cli: &Cli) -> Result<CommandOutput, AppError> {
    trace!("execute_command start");
    match &cli.command {
        Command::Ping => Ok(CommandOutput::Ping),
        Command::Version => Ok(CommandOutput::Version {
            api_version: env!("CARGO_PKG_VERSION"),
            schema_version: db::migrate::current_schema_version(),
            build: "dev",
        }),
        Command::Tags
        | Command::Feeds { .. }
        | Command::Sync
        | Command::List { .. }
        | Command::View { .. }
        | Command::Mark { .. } => {
            let config = load_config(cli)?;
            debug!(
                db_path = ?config.database.path,
                feeds_path = ?config.feeds.source,
                "loaded configuration"
            );
            let mut store = db::sqlite::SqliteStore::open(&config.database.path)?;
            store.migrate()?;

            match &cli.command {
                Command::Tags => {
                    let tag_manager = TagManager::new(&store);
                    let tags = tag_manager.list_tags()?;
                    Ok(CommandOutput::Tags { tags })
                }
                Command::Feeds { config_check } => {
                    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
                    debug_assert!(!config_check);
                    feed::reconcile_feeds(&store, &feeds_config, &config.unread_tag)?;
                    let db_feeds = store.list_feeds()?;
                    let feeds = feed::render_feed_list(&feeds_config, &db_feeds);
                    Ok(CommandOutput::FeedsList { feeds })
                }
                Command::Sync => {
                    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
                    let summary = sync::run_sync(&mut store, &config, &feeds_config)?;
                    Ok(CommandOutput::Sync { summary })
                }
                Command::List {
                    query,
                    sort,
                    limit,
                    cursor,
                } => {
                    let query = EntryQuery::parse(query.as_deref(), &config.unread_tag)?;
                    let sort = sort.unwrap_or(SortOrder::FirstSeenDesc);
                    let limit = resolve_list_limit(*limit, config.query)?;
                    let list = entry::list_entries(&store, &query, sort, limit, cursor.as_deref())?;
                    Ok(CommandOutput::List { list })
                }
                Command::View { id } => {
                    let detail = entry::view_entry(&store, &config, *id)?;
                    Ok(CommandOutput::View { detail })
                }
                Command::Mark { command } => {
                    let updated = execute_mark(&mut store, &config, command)?;
                    Ok(CommandOutput::Mark { updated })
                }
                Command::Ping | Command::Version => unreachable!("handled above"),
            }
        }
    }
}

/// Resolves the effective list limit from CLI argument and query config.
fn resolve_list_limit(limit: Option<usize>, query: config::QueryConfig) -> Result<usize, AppError> {
    let resolved = limit.unwrap_or(query.default_limit);
    if resolved == 0 {
        return Err(AppError::invalid_query("--limit must be greater than 0"));
    }
    if resolved > query.max_limit {
        return Err(AppError::invalid_query(format!(
            "--limit must be less than or equal to {}",
            query.max_limit
        )));
    }
    Ok(resolved)
}

/// Renders JSON output for a command result.
fn render_json(result: &CommandOutput) -> Result<(), AppError> {
    let data = match result {
        CommandOutput::Ping => json!({ "ok": true }),
        CommandOutput::Version {
            api_version,
            schema_version,
            build,
        } => json!({
            "api_version": api_version,
            "schema_version": schema_version,
            "build": build,
        }),
        CommandOutput::Tags { tags } => json!({ "tags": tags }),
        CommandOutput::FeedsList { feeds } => serde_json::to_value(feeds)?,
        CommandOutput::Sync { summary } => serde_json::to_value(summary)?,
        CommandOutput::List { list } => serde_json::to_value(list)?,
        CommandOutput::View { detail } => serde_json::to_value(detail)?,
        CommandOutput::Mark { updated } => json!({ "updated": updated }),
    };

    print_json_or_fallback(&Envelope::ok(data));
    Ok(())
}

/// Renders human-readable output for a command result.
fn render_plain(result: &CommandOutput) {
    match result {
        CommandOutput::Ping => println!("ok"),
        CommandOutput::Version {
            api_version,
            schema_version,
            build,
        } => println!("api_version={api_version} schema_version={schema_version} build={build}"),
        CommandOutput::Tags { tags } => {
            for tag in tags {
                println!("{tag}");
            }
        }
        CommandOutput::FeedsList { feeds } => {
            for feed in &feeds.feeds {
                let title = feed.title.as_deref().unwrap_or("(untitled)");
                let tags = format_tags(&feed.tags);
                if tags.is_empty() {
                    println!("[{}] {} ({})", feed.id, title, feed.feed_key);
                } else {
                    println!("[{}] {} ({}) [{}]", feed.id, title, feed.feed_key, tags);
                }
                println!("  url: {}", feed.url);
                if let Some(site_url) = &feed.site_url {
                    println!("  site: {site_url}");
                }
                if let Some(author) = &feed.author {
                    println!("  author: {author}");
                }
            }
        }
        CommandOutput::Sync { summary } => {
            println!("status: {}", summary.status.as_str());
            println!(
                "fetched: {} failed: {} new_entries: {} elapsed: {:.2}s",
                summary.fetched, summary.failed, summary.new_entries, summary.elapsed
            );
            if !summary.errors.is_empty() {
                println!("errors: {}", summary.errors.len());
                for error in &summary.errors {
                    println!(
                        "  {} {} retry={}",
                        error.feed_url,
                        error.code.as_str(),
                        error.retry
                    );
                    println!("    {}", error.message);
                }
            }
        }
        CommandOutput::List { list } => {
            println!("total: {}", list.total_hits);
            if let Some(cursor) = &list.next_cursor {
                println!("next_cursor: {cursor}");
            }
            for entry in &list.items {
                let title = entry.title.as_deref().unwrap_or("(untitled)");
                let tags = format_tags(&entry.tags);
                if tags.is_empty() {
                    println!("[{}] {title}", entry.id);
                } else {
                    println!("[{}] {title} [{tags}]", entry.id);
                }
            }
        }
        CommandOutput::View { detail } => {
            let title = detail.title.as_deref().unwrap_or("(untitled)");
            println!("[{}] {title}", detail.id);
            if let Some(feed_title) = &detail.feed_title {
                println!("feed: {feed_title} (id: {})", detail.feed_id);
            } else {
                println!("feed_id: {}", detail.feed_id);
            }
            if let Some(author) = &detail.author {
                println!("author: {author}");
            }
            if let Some(link) = &detail.link {
                println!("link: {link}");
            }
            if !detail.tags.is_empty() {
                println!("tags: {}", format_tags(&detail.tags));
            }
            if let Some(published) = detail.published_at {
                println!("published_at: {published}");
            }
            println!("first_seen_at: {}", detail.first_seen_at);
            if let Some(content) = &detail.content {
                println!();
                println!("{content}");
            }
        }
        CommandOutput::Mark { updated } => println!("updated: {updated}"),
    }
}

/// Renders human-readable output for feeds config validation.
fn render_config_check_plain(report: &ConfigCheckReport) {
    println!("valid: {}", report.valid);
    println!("checked_feeds: {}", report.checked_feeds);
    println!("errors: {}", report.errors.len());
    for issue in &report.errors {
        if let Some(path) = &issue.path {
            println!("  {} {} ({path})", issue.code, issue.message);
        } else {
            println!("  {} {}", issue.code, issue.message);
        }
    }
    println!("warnings: {}", report.warnings.len());
    for issue in &report.warnings {
        if let Some(path) = &issue.path {
            println!("  {} {} ({path})", issue.code, issue.message);
        } else {
            println!("  {} {}", issue.code, issue.message);
        }
    }
}

fn format_tags(tags: &[String]) -> String {
    tags.join(", ")
}

/// Prints JSON to stdout, falling back to a hard-coded INTERNAL error JSON on failure.
fn print_json_or_fallback<T: serde::Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(json) => println!("{json}"),
        Err(_) => println!("{FALLBACK_INTERNAL_ERROR_JSON}"),
    }
}

/// Fallback JSON printed when JSON serialization fails unexpectedly.
const FALLBACK_INTERNAL_ERROR_JSON: &str = "{\"ok\":false,\"data\":null,\"error\":{\"code\":\"INTERNAL\",\"message\":\"Failed to serialize response\",\"retry\":false}}";

/// Loads config and applies CLI overrides.
fn load_config(cli: &Cli) -> Result<config::AppConfig, AppError> {
    let mut config = config::AppConfig::load(cli.config.clone())?;
    if let Some(root_dir) = cli.root_dir.clone() {
        config.override_root_dir(root_dir)?;
    }
    Ok(config)
}

/// Resolves effective output format (CLI > config > default).
fn resolve_output(cli: &Cli, config: Option<&config::AppConfig>) -> OutputFormat {
    if let Some(output) = cli.output {
        return output;
    }
    if let Some(config) = config {
        return config.cli.output;
    }
    OutputFormat::Plain
}

/// Resolves the effective output format using CLI or config when available.
fn resolve_effective_output(cli: &Cli) -> OutputFormat {
    if let Some(output) = cli.output {
        return output;
    }
    match cli.command {
        Command::Ping | Command::Version => OutputFormat::Plain,
        _ => match load_config(cli) {
            Ok(config) => resolve_output(cli, Some(&config)),
            Err(_) => OutputFormat::Plain,
        },
    }
}

/// Handles CLI parse errors and prints appropriate output.
fn handle_cli_parse_error(args: &[OsString], error: clap::Error) -> ExitCode {
    use clap::error::ErrorKind;
    let output = detect_output_from_args(args);
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = error.print();
            ExitCode::SUCCESS
        }
        _ => {
            match output {
                OutputFormat::Json => {
                    let app_error = AppError::config(error.to_string());
                    print_json_or_fallback(&Envelope::<serde_json::Value>::fatal(&app_error));
                }
                OutputFormat::Plain => eprintln!("{error}"),
            }
            ExitCode::from(1)
        }
    }
}

/// Detects output format from raw CLI args.
fn detect_output_from_args(args: &[OsString]) -> OutputFormat {
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        let arg_value = arg.to_string_lossy();
        if arg_value == "--output" {
            if let Some(value) = iter.peek() {
                return if value.to_string_lossy() == "plain" {
                    OutputFormat::Plain
                } else {
                    OutputFormat::Json
                };
            }
            return OutputFormat::Json;
        }
        if let Some(value) = arg_value.strip_prefix("--output=") {
            return if value == "plain" {
                OutputFormat::Plain
            } else {
                OutputFormat::Json
            };
        }
    }
    OutputFormat::Plain
}

/// Prints error diagnostics to stderr when debug/trace is enabled.
fn maybe_print_diagnostics(cli: &Cli, error: &AppError) {
    let level = resolve_effective_log_level(cli);
    if !should_emit_diagnostics(level) {
        return;
    }
    eprintln!("error: {error}");
    let mut current = error.source();
    let mut depth = 0usize;
    while let Some(source) = current {
        depth += 1;
        eprintln!("caused by[{depth}]: {source}");
        current = source.source();
    }
}

/// Resolves effective log level (CLI > config > default).
fn resolve_effective_log_level(cli: &Cli) -> config::LogLevel {
    if cli.trace {
        return config::LogLevel::Trace;
    }
    if cli.debug {
        return config::LogLevel::Debug;
    }
    match load_config(cli) {
        Ok(config) => config.log.level,
        Err(_) => config::LogLevel::Info,
    }
}

/// Returns true if diagnostics should be emitted for the log level.
fn should_emit_diagnostics(level: config::LogLevel) -> bool {
    matches!(level, config::LogLevel::Debug | config::LogLevel::Trace)
}

/// Initializes stderr logging for diagnostics.
fn init_logging(level: config::LogLevel) {
    if !should_emit_diagnostics(level) {
        return;
    }
    let level = match level {
        config::LogLevel::Error => tracing::Level::ERROR,
        config::LogLevel::Warn => tracing::Level::WARN,
        config::LogLevel::Info => tracing::Level::INFO,
        config::LogLevel::Debug => tracing::Level::DEBUG,
        config::LogLevel::Trace => tracing::Level::TRACE,
    };
    let _ = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(io::stderr)
        .with_ansi(false)
        .try_init();
}

fn execute_mark(
    store: &mut db::sqlite::SqliteStore,
    config: &config::AppConfig,
    command: &MarkCommand,
) -> Result<usize, AppError> {
    match command {
        MarkCommand::Read { ids } => {
            entry::mark_entries(store, ids, &[], std::slice::from_ref(&config.unread_tag))
        }
        MarkCommand::Unread { ids } => {
            entry::mark_entries(store, ids, std::slice::from_ref(&config.unread_tag), &[])
        }
        MarkCommand::Tag { ids, add, remove } => {
            let add_tags = parse_tag_list(add.as_deref());
            let remove_tags = parse_tag_list(remove.as_deref());
            entry::mark_entries(store, ids, &add_tags, &remove_tags)
        }
    }
}

fn parse_tag_list(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for part in raw.split(',') {
        let tag = part.trim();
        if tag.is_empty() {
            continue;
        }
        if seen.insert(tag.to_string()) {
            tags.push(tag.to_string());
        }
    }
    tags
}
