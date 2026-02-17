//! Picofeedr CLI entrypoint.

use clap::Parser;
use picofeedr::cli::{Cli, Command, MarkCommand, OutputFormat, SortOrder};
use picofeedr::config;
use picofeedr::config::feeds::ConfigCheckReport;
use picofeedr::db;
use picofeedr::entry;
use picofeedr::entry::{EntryDetail, EntryListResponse};
use picofeedr::error::{AppError, ErrorDetails, error_details};
use picofeedr::feed;
use picofeedr::feed::FeedListResponse;
use picofeedr::query::EntryQuery;
use picofeedr::response::{
    Envelope, MarkResult, PingResult, ResponseStatus, TagsResult, VersionResult,
};
use picofeedr::status::StatusResponse;
use picofeedr::sync;
use picofeedr::sync::SyncSummary;
use picofeedr::tag::TagManager;
use picofeedr::time;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;
use tracing::{debug, trace};

/// Execution results for CLI commands.
enum CommandOutput {
    Ping,
    Version {
        api_version: &'static str,
        db_schema_version: i64,
        build: &'static str,
    },
    Tags {
        tags: Vec<String>,
    },
    Status {
        status: StatusResponse,
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

/// Runtime failure category for command execution and output rendering.
enum RunFailure {
    /// Domain/application level failure.
    App(AppError),
    /// Stdout write failure while rendering output.
    Io(io::Error),
}

impl From<AppError> for RunFailure {
    /// Converts an [`AppError`] into [`RunFailure`].
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

impl From<io::Error> for RunFailure {
    /// Converts an [`io::Error`] into [`RunFailure`].
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RunFailure {
    /// Converts a JSON serialization error into [`RunFailure`].
    fn from(error: serde_json::Error) -> Self {
        Self::App(AppError::from(error))
    }
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
            Err(RunFailure::Io(error)) => return handle_output_error(&cli, error),
            Err(RunFailure::App(error)) => {
                maybe_print_diagnostics(&cli, &error);
                match output {
                    OutputFormat::Json => {
                        match print_json_or_fallback(&Envelope::<()>::fatal(&error)) {
                            Ok(()) => {}
                            Err(write_error) => return handle_output_error(&cli, write_error),
                        }
                    }
                    OutputFormat::Plain => eprintln!("{error}"),
                }
                return ExitCode::from(1);
            }
        }
    }
    if let Err(error) = run(&cli, output) {
        match error {
            RunFailure::Io(error) => return handle_output_error(&cli, error),
            RunFailure::App(error) => {
                maybe_print_diagnostics(&cli, &error);
                match output {
                    OutputFormat::Json => {
                        match print_json_or_fallback(&Envelope::<()>::fatal(&error)) {
                            Ok(()) => {}
                            Err(write_error) => return handle_output_error(&cli, write_error),
                        }
                    }
                    OutputFormat::Plain => eprintln!("{error}"),
                }
                return ExitCode::from(1);
            }
        }
    }
    ExitCode::SUCCESS
}

/// Executes the CLI command and prints a JSON response.
fn run(cli: &Cli, output: OutputFormat) -> Result<(), RunFailure> {
    let result = if matches!((&cli.command, output), (Command::Sync, OutputFormat::Plain)) {
        execute_sync_command_plain(cli)?
    } else {
        execute_command(cli)?
    };
    match output {
        OutputFormat::Json => render_json(&result)?,
        OutputFormat::Plain => render_plain(&result)?,
    }
    Ok(())
}

/// Runs static feeds config validation without touching the database.
fn run_config_check(cli: &Cli, output: OutputFormat) -> Result<ExitCode, RunFailure> {
    let config = load_config(cli)?;
    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
    let report = feeds_config.validate();
    let is_valid = report.valid;
    match output {
        OutputFormat::Json => {
            let status = if is_valid {
                ResponseStatus::Ok
            } else {
                ResponseStatus::Warning
            };
            print_json_or_fallback(&Envelope::ok_with_status(report, status))?;
        }
        OutputFormat::Plain => render_config_check_plain(&report)?,
    }
    Ok(if is_valid {
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
            db_schema_version: db::migrate::current_schema_version(),
            build: "dev",
        }),
        Command::Tags
        | Command::Status
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
                Command::Status => {
                    let meta = store.read_system_meta()?;
                    let status = StatusResponse::from_meta(
                        &meta,
                        db::migrate::current_schema_version(),
                        env!("CARGO_PKG_VERSION"),
                    );
                    Ok(CommandOutput::Status { status })
                }
                Command::Feeds { config_check } => {
                    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
                    debug_assert!(!config_check);
                    feed::reconcile_feeds(&mut store, &feeds_config, &config.unread_tag)?;
                    let db_feeds = store.list_feeds()?;
                    let feeds = feed::render_feed_list(&feeds_config, &db_feeds);
                    store.bump_revision(time::current_epoch())?;
                    Ok(CommandOutput::FeedsList { feeds })
                }
                Command::Sync => execute_sync_with_store(&config, &mut store),
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
                    let detail = entry::view_entry(&store, &config, id)?;
                    Ok(CommandOutput::View { detail })
                }
                Command::Mark { command } => {
                    let updated = execute_mark(&mut store, &config, command)?;
                    store.bump_revision(time::current_epoch())?;
                    Ok(CommandOutput::Mark { updated })
                }
                Command::Ping | Command::Version => unreachable!("handled above"),
            }
        }
    }
}

/// Executes sync command using the shared store path without progress rendering.
fn execute_sync_with_store(
    config: &config::AppConfig,
    store: &mut db::sqlite::SqliteStore,
) -> Result<CommandOutput, AppError> {
    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
    let summary = sync::run_sync(store, config, &feeds_config)?;
    let now = time::current_epoch();
    store.bump_revision(now)?;
    store.update_sync(now, summary.status.as_str())?;
    Ok(CommandOutput::Sync { summary })
}

/// Executes sync command and streams plain progress lines to stdout.
fn execute_sync_command_plain(cli: &Cli) -> Result<CommandOutput, RunFailure> {
    let config = load_config(cli)?;
    debug!(
        db_path = ?config.database.path,
        feeds_path = ?config.feeds.source,
        "loaded configuration"
    );
    let mut store = db::sqlite::SqliteStore::open(&config.database.path)?;
    store.migrate()?;
    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;

    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    let mut write_error: Option<io::Error> = None;
    let mut on_progress = |event: sync::SyncProgressEvent| {
        if write_error.is_some() {
            return;
        }
        if let Err(error) = render_sync_progress_line(&mut writer, &event) {
            write_error = Some(error);
            return;
        }
        if let Err(error) = writer.flush() {
            write_error = Some(error);
        }
    };

    let summary =
        sync::run_sync_with_progress(&mut store, &config, &feeds_config, Some(&mut on_progress))?;
    if let Some(error) = write_error {
        return Err(RunFailure::Io(error));
    }

    let now = time::current_epoch();
    store.bump_revision(now)?;
    store.update_sync(now, summary.status.as_str())?;
    Ok(CommandOutput::Sync { summary })
}

/// Resolves the effective list limit from CLI argument and query config.
fn resolve_list_limit(limit: Option<usize>, query: config::QueryConfig) -> Result<usize, AppError> {
    let resolved = limit.unwrap_or(query.default_limit);
    if resolved == 0 {
        return Err(AppError::invalid_query_with_details(
            "--limit must be greater than 0",
            limit_error_details("zero_or_negative", resolved, query.max_limit),
        ));
    }
    if resolved > query.max_limit {
        return Err(AppError::invalid_query_with_details(
            format!("--limit must be less than or equal to {}", query.max_limit),
            limit_error_details("exceeds_max_limit", resolved, query.max_limit),
        ));
    }
    Ok(resolved)
}

/// Builds standardized details payload for limit validation failures.
fn limit_error_details(kind: &str, value: usize, _max_limit: usize) -> ErrorDetails {
    let hint = match kind {
        "zero_or_negative" => "limit_must_be_greater_than_zero",
        "exceeds_max_limit" => "limit_exceeds_configured_max_limit",
        _ => "invalid_limit",
    };
    error_details([
        ("kind", Value::from("limit_out_of_range")),
        ("field", Value::from("limit")),
        ("value", Value::from(value)),
        ("hint", Value::from(hint)),
    ])
}

/// Renders JSON output for a command result.
fn render_json(result: &CommandOutput) -> Result<(), RunFailure> {
    fn print_success<T: serde::Serialize>(
        data: T,
        status: ResponseStatus,
    ) -> Result<(), RunFailure> {
        print_json_or_fallback(&Envelope::ok_with_status(data, status))?;
        Ok(())
    }

    match result {
        CommandOutput::Ping => print_success(PingResult::ok(), ResponseStatus::Ok),
        CommandOutput::Version {
            api_version,
            db_schema_version,
            build,
        } => print_success(
            VersionResult {
                api_version: (*api_version).to_string(),
                db_schema_version: *db_schema_version,
                build: (*build).to_string(),
            },
            ResponseStatus::Ok,
        ),
        CommandOutput::Tags { tags } => {
            print_success(TagsResult { tags: tags.clone() }, ResponseStatus::Ok)
        }
        CommandOutput::Status { status } => print_success(status, ResponseStatus::Ok),
        CommandOutput::FeedsList { feeds } => print_success(feeds, ResponseStatus::Ok),
        CommandOutput::Sync { summary } => {
            let envelope_status = if matches!(summary.status, sync::SyncStatus::Completed) {
                ResponseStatus::Ok
            } else {
                ResponseStatus::Warning
            };
            print_success(summary, envelope_status)
        }
        CommandOutput::List { list } => print_success(list, ResponseStatus::Ok),
        CommandOutput::View { detail } => print_success(detail, ResponseStatus::Ok),
        CommandOutput::Mark { updated } => print_success(
            MarkResult {
                updated_entry_count: *updated,
            },
            ResponseStatus::Ok,
        ),
    }
}

/// Renders human-readable output for a command result.
fn render_plain(result: &CommandOutput) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    match result {
        CommandOutput::Ping => writeln!(writer, "ok")?,
        CommandOutput::Version {
            api_version,
            db_schema_version,
            build,
        } => writeln!(
            writer,
            "api_version={api_version} db_schema_version={db_schema_version} build={build}"
        )?,
        CommandOutput::Tags { tags } => {
            for tag in tags {
                writeln!(writer, "{tag}")?;
            }
        }
        CommandOutput::Status { status } => {
            writeln!(writer, "revision: {}", status.revision)?;
            writeln!(
                writer,
                "last_write_at: {}",
                status
                    .last_write_at
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_string())
            )?;
            writeln!(writer, "db_schema_version: {}", status.db_schema_version)?;
            writeln!(writer, "api_version: {}", status.api_version)?;
            writeln!(
                writer,
                "last_sync_at: {}",
                status
                    .last_sync_at
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_string())
            )?;
            writeln!(
                writer,
                "last_sync_status: {}",
                status.last_sync_status.as_deref().unwrap_or("null")
            )?;
        }
        CommandOutput::FeedsList { feeds } => {
            for feed in &feeds.feeds {
                let title = feed.title.as_deref().unwrap_or("(untitled)");
                let tags = format_tags(&feed.tags);
                if tags.is_empty() {
                    writeln!(writer, "[{}] {}", feed.feed_id, title)?;
                } else {
                    writeln!(writer, "[{}] {} [{}]", feed.feed_id, title, tags)?;
                }
                writeln!(writer, "  url: {}", feed.url)?;
                if let Some(site_url) = &feed.site_url {
                    writeln!(writer, "  site: {site_url}")?;
                }
                if let Some(author) = &feed.author {
                    writeln!(writer, "  author: {author}")?;
                }
            }
        }
        CommandOutput::Sync { summary } => {
            writeln!(writer, "status: {}", summary.status.as_str())?;
            writeln!(
                writer,
                "fetched_feed_count: {} failed_feed_count: {} new_entry_count: {} duration_ms: {}",
                summary.fetched_feed_count,
                summary.failed_feed_count,
                summary.new_entry_count,
                summary.duration_ms
            )?;
            if !summary.errors.is_empty() {
                writeln!(writer, "errors: {}", summary.errors.len())?;
                for error in &summary.errors {
                    writeln!(
                        writer,
                        "  {} {} retryable={}",
                        error.feed_url,
                        error.code.as_str(),
                        error.retryable
                    )?;
                    writeln!(writer, "    {}", error.message)?;
                }
            }
        }
        CommandOutput::List { list } => {
            writeln!(writer, "total_count: {}", list.total_count)?;
            if let Some(cursor) = &list.next_page_token {
                writeln!(writer, "next_page_token: {cursor}")?;
            }
            let feed_titles = list
                .feeds
                .iter()
                .map(|feed| {
                    (
                        feed.feed_id.clone(),
                        feed.title.as_deref().unwrap_or("(untitled)").to_string(),
                    )
                })
                .collect::<HashMap<_, _>>();
            for entry in &list.items {
                let title = entry.title.as_deref().unwrap_or("(untitled)");
                let feed_title = feed_titles
                    .get(&entry.feed_id)
                    .map(String::as_str)
                    .unwrap_or("(unknown)");
                let tags = format_tags(&entry.tags);
                if tags.is_empty() {
                    writeln!(writer, "[{}] {title} ({feed_title})", entry.entry_id)?;
                } else {
                    writeln!(
                        writer,
                        "[{}] {title} ({feed_title}) [{tags}]",
                        entry.entry_id
                    )?;
                }
            }
        }
        CommandOutput::View { detail } => {
            let title = detail.title.as_deref().unwrap_or("(untitled)");
            writeln!(writer, "{} {title}", detail.entry_id)?;
            if let Some(feed_title) = &detail.feed_title {
                writeln!(writer, "feed: {feed_title} (id: {})", detail.feed_id)?;
            } else {
                writeln!(writer, "feed_id: {}", detail.feed_id)?;
            }
            if let Some(author) = &detail.author {
                writeln!(writer, "author: {author}")?;
            }
            if let Some(link) = &detail.link {
                writeln!(writer, "link: {link}")?;
            }
            if !detail.tags.is_empty() {
                writeln!(writer, "tags: {}", format_tags(&detail.tags))?;
            }
            if let Some(published) = detail.published_at {
                writeln!(writer, "published_at: {published}")?;
            }
            writeln!(writer, "first_seen_at: {}", detail.first_seen_at)?;
            if let Some(content) = &detail.content {
                writeln!(writer)?;
                writeln!(writer, "{content}")?;
            }
        }
        CommandOutput::Mark { updated } => writeln!(writer, "updated_entry_count: {updated}")?,
    }
    writer.flush()?;
    Ok(())
}

/// Renders one sync progress event as a plain output line.
fn render_sync_progress_line<W: Write>(
    writer: &mut W,
    event: &sync::SyncProgressEvent,
) -> io::Result<()> {
    match event {
        sync::SyncProgressEvent::Start { total_feeds } => {
            writeln!(writer, "sync:start total_feeds={total_feeds}")
        }
        sync::SyncProgressEvent::FeedStart {
            index,
            total_feeds,
            url,
        } => writeln!(
            writer,
            "sync:feed start index={index}/{total_feeds} url={url}"
        ),
        sync::SyncProgressEvent::FeedOk {
            index,
            total_feeds,
            url,
            entries,
        } => writeln!(
            writer,
            "sync:feed ok index={index}/{total_feeds} url={url} entries={entries}"
        ),
        sync::SyncProgressEvent::FeedError {
            index,
            total_feeds,
            url,
            code,
            retryable,
        } => writeln!(
            writer,
            "sync:feed error index={index}/{total_feeds} url={url} code={} retryable={retryable}",
            code.as_str()
        ),
    }
}

/// Renders human-readable output for feeds config validation.
fn render_config_check_plain(report: &ConfigCheckReport) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    writeln!(writer, "valid: {}", report.valid)?;
    writeln!(writer, "checked_feeds: {}", report.checked_feeds)?;
    writeln!(writer, "errors: {}", report.errors.len())?;
    for issue in &report.errors {
        if let Some(path) = &issue.path {
            writeln!(writer, "  {} {} ({path})", issue.code, issue.message)?;
        } else {
            writeln!(writer, "  {} {}", issue.code, issue.message)?;
        }
    }
    writeln!(writer, "warnings: {}", report.warnings.len())?;
    for issue in &report.warnings {
        if let Some(path) = &issue.path {
            writeln!(writer, "  {} {} ({path})", issue.code, issue.message)?;
        } else {
            writeln!(writer, "  {} {}", issue.code, issue.message)?;
        }
    }
    writer.flush()?;
    Ok(())
}

/// Formats tags for plain output.
fn format_tags(tags: &[String]) -> String {
    tags.join(", ")
}

/// Prints JSON to stdout, falling back to a hard-coded INTERNAL error JSON on failure.
fn print_json_or_fallback<T: serde::Serialize>(value: &T) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    match serde_json::to_string(value) {
        Ok(json) => writeln!(writer, "{json}")?,
        Err(_) => writeln!(writer, "{FALLBACK_INTERNAL_ERROR_JSON}")?,
    }
    writer.flush()
}

/// Fallback JSON printed when JSON serialization fails unexpectedly.
const FALLBACK_INTERNAL_ERROR_JSON: &str = "{\"status\":\"error\",\"result\":null,\"error\":{\"code\":\"INTERNAL\",\"message\":\"Failed to serialize response\",\"retryable\":false,\"details\":null},\"meta\":{\"api_version\":\"unknown\",\"db_schema_version\":0,\"generated_at\":0}}";

/// Loads config and applies CLI overrides.
fn load_config(cli: &Cli) -> Result<config::AppConfig, AppError> {
    let mut config = config::AppConfig::load(cli.config.clone())?;
    if let Some(root_dir) = cli.storage_root.clone() {
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
                    if let Err(write_error) =
                        print_json_or_fallback(&Envelope::<()>::fatal(&app_error))
                    {
                        if is_broken_pipe_error(&write_error) {
                            return ExitCode::SUCCESS;
                        }
                        eprintln!("failed to write CLI output: {write_error}");
                    }
                }
                OutputFormat::Plain => eprintln!("{error}"),
            }
            ExitCode::from(1)
        }
    }
}

/// Handles stdout output errors and maps broken pipes to successful termination.
fn handle_output_error(cli: &Cli, error: io::Error) -> ExitCode {
    if is_broken_pipe_error(&error) {
        if should_emit_diagnostics(resolve_effective_log_level(cli)) {
            eprintln!("stdout closed by downstream consumer (broken pipe)");
        }
        return ExitCode::SUCCESS;
    }
    let app_error = AppError::io_with_source("failed to write CLI output", error);
    maybe_print_diagnostics(cli, &app_error);
    eprintln!("{app_error}");
    ExitCode::from(1)
}

/// Returns true when the I/O error was caused by a broken stdout pipe.
fn is_broken_pipe_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::BrokenPipe
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
