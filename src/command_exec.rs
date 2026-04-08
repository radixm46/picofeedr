//! Command execution orchestration for CLI runtime.

use crate::{CommandOutput, RunFailure, output};
use picofeedr::cli::{Cli, Command, MarkCommand, SortOrder};
use picofeedr::config;
use picofeedr::db;
use picofeedr::entry;
use picofeedr::error::{AppError, ErrorDetails, error_details};
use picofeedr::feed;
use picofeedr::query::EntryQuery;
use picofeedr::response::{MarkResponse, PingResponse, TagListResponse, VersionResponse};
use picofeedr::status::StatusResponse;
use picofeedr::sync;
use picofeedr::{TagManager, current_epoch, parse_tag_csv};
use serde_json::Value;
use std::io::{self, Write};
use tracing::{debug, trace};

/// Executes the CLI command and returns the result.
pub(crate) fn run_command(
    cli: &Cli,
    config: &config::AppConfig,
) -> Result<CommandOutput, AppError> {
    trace!("run_command start");
    match &cli.command {
        Command::Ping => Ok(CommandOutput::Ping(PingResponse::ok())),
        Command::Version => Ok(CommandOutput::Version(VersionResponse {
            api_version: env!("CARGO_PKG_VERSION").to_string(),
            db_schema_version: db::migrate::current_schema_version(),
            build: "dev".to_string(),
        })),
        Command::Tags
        | Command::Status
        | Command::Feeds { .. }
        | Command::Sync
        | Command::List { .. }
        | Command::View { .. }
        | Command::Mark { .. } => {
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
                    Ok(CommandOutput::Tags(TagListResponse { tags }))
                }
                Command::Status => {
                    let meta = store.read_system_meta()?;
                    let status = StatusResponse::from_system_meta(
                        &meta,
                        db::migrate::current_schema_version(),
                        env!("CARGO_PKG_VERSION"),
                    );
                    Ok(CommandOutput::Status(status))
                }
                Command::Feeds { config_check } => {
                    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
                    debug_assert!(!config_check);
                    feed::reconcile_feeds(&mut store, &feeds_config, &config.unread_tag)?;
                    let db_feeds = store.list_feeds()?;
                    let feeds = feed::build_feed_list_response(&feeds_config, &db_feeds);
                    store.bump_revision(current_epoch())?;
                    Ok(CommandOutput::FeedsList(feeds))
                }
                Command::Sync => run_sync_with_store(config, &mut store),
                Command::List {
                    query,
                    sort,
                    limit,
                    cursor,
                    id,
                } => {
                    let query = EntryQuery::parse(query.as_deref(), &config.unread_tag)?;
                    let sort = sort.unwrap_or(SortOrder::FirstSeenDesc);
                    let limit = resolve_list_limit(*limit, config.query)?;
                    let list = entry::list_entries(&store, &query, sort, limit, cursor.as_deref())?;
                    Ok(CommandOutput::List {
                        list,
                        include_id: *id,
                    })
                }
                Command::View { id } => {
                    let detail = entry::view_entry(&store, config, id)?;
                    Ok(CommandOutput::View(detail))
                }
                Command::Mark { command } => {
                    let updated = run_mark_command(&mut store, config, command)?;
                    store.bump_revision(current_epoch())?;
                    Ok(CommandOutput::Mark(MarkResponse {
                        updated_entry_count: updated,
                    }))
                }
                Command::Ping | Command::Version => unreachable!("handled above"),
            }
        }
    }
}

/// Executes sync command and streams plain progress lines to stdout.
pub(crate) fn run_sync_command_plain(
    config: &config::AppConfig,
) -> Result<CommandOutput, RunFailure> {
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
        if let Err(error) = output::write_sync_progress_line(&mut writer, &event) {
            write_error = Some(error);
            return;
        }
        if let Err(error) = writer.flush() {
            write_error = Some(error);
        }
    };

    let summary =
        sync::run_sync_with_progress(&mut store, config, &feeds_config, Some(&mut on_progress))?;
    if let Some(error) = write_error {
        return Err(RunFailure::Io(error));
    }

    let now = current_epoch();
    store.bump_revision(now)?;
    store.update_sync(now, summary.status.as_str())?;
    Ok(CommandOutput::Sync(summary))
}

/// Executes sync command using the shared store path without progress rendering.
fn run_sync_with_store(
    config: &config::AppConfig,
    store: &mut db::sqlite::SqliteStore,
) -> Result<CommandOutput, AppError> {
    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
    let summary = sync::run_sync(store, config, &feeds_config)?;
    let now = current_epoch();
    store.bump_revision(now)?;
    store.update_sync(now, summary.status.as_str())?;
    Ok(CommandOutput::Sync(summary))
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

fn run_mark_command(
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
    parse_tag_csv(raw)
}
