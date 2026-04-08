//! Command execution orchestration for CLI runtime.

use crate::{RunFailure, output};
use picofeedr::cli::{Cli, Command, MarkCommand, SortOrder};
use picofeedr::config;
use picofeedr::db;
use picofeedr::entry::{self, EntryDetail, EntryListResponse};
use picofeedr::error::{AppError, ErrorDetails, error_details};
use picofeedr::feed::{self, FeedListResponse};
use picofeedr::query::EntryQuery;
use picofeedr::response::{MarkResponse, TagListResponse};
use picofeedr::status::StatusResponse;
use picofeedr::sync::{self, SyncSummary};
use picofeedr::{TagManager, current_epoch, parse_tag_csv};
use serde_json::Value;
use std::io::{self, Write};
use tracing::debug;

pub(crate) fn run_plain_command(
    cli: &Cli,
    config: &config::AppConfig,
) -> Result<output::PlainOutput, RunFailure> {
    match &cli.command {
        Command::Tags => Ok(output::PlainOutput::Tags(load_tags_response(config)?.tags)),
        Command::Status => Ok(output::PlainOutput::Status(load_status_response(config)?)),
        Command::Feeds { .. } => Ok(output::PlainOutput::Feeds(run_feeds_command(config)?)),
        Command::Sync => run_sync_command_plain(config),
        Command::List {
            query,
            sort,
            limit,
            cursor,
            id,
        } => Ok(output::PlainOutput::List {
            list: run_list_command(config, query.as_deref(), *sort, *limit, cursor.as_deref())?,
            include_id: *id,
        }),
        Command::View { id } => Ok(output::PlainOutput::View(run_view_command(config, id)?)),
        Command::Mark { command } => Ok(output::PlainOutput::Mark(run_mark_response(
            config, command,
        )?)),
        Command::Ping | Command::Version => unreachable!("handled in main"),
    }
}

pub(crate) fn load_tags_response(config: &config::AppConfig) -> Result<TagListResponse, AppError> {
    with_store(config, |store| {
        let tag_manager = TagManager::new(store);
        let tags = tag_manager.list_tags()?;
        Ok(TagListResponse { tags })
    })
}

pub(crate) fn load_status_response(config: &config::AppConfig) -> Result<StatusResponse, AppError> {
    with_store(config, |store| {
        let meta = store.read_system_meta()?;
        Ok(StatusResponse::from_system_meta(
            &meta,
            db::migrate::current_schema_version(),
            env!("CARGO_PKG_VERSION"),
        ))
    })
}

pub(crate) fn run_feeds_command(config: &config::AppConfig) -> Result<FeedListResponse, AppError> {
    with_store(config, |store| {
        let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
        feed::reconcile_feeds(store, &feeds_config, &config.unread_tag)?;
        let db_feeds = store.list_feeds()?;
        let feeds = feed::build_feed_list_response(&feeds_config, &db_feeds);
        store.bump_revision(current_epoch())?;
        Ok(feeds)
    })
}

pub(crate) fn run_sync_command(config: &config::AppConfig) -> Result<SyncSummary, AppError> {
    with_store(config, |store| run_sync_with_store(config, store))
}

pub(crate) fn run_list_command(
    config: &config::AppConfig,
    query: Option<&str>,
    sort: Option<SortOrder>,
    limit: Option<usize>,
    cursor: Option<&str>,
) -> Result<EntryListResponse, AppError> {
    with_store(config, |store| {
        let query = EntryQuery::parse(query, &config.unread_tag)?;
        let sort = sort.unwrap_or(SortOrder::FirstSeenDesc);
        let limit = resolve_list_limit(limit, config.query)?;
        entry::list_entries(store, &query, sort, limit, cursor)
    })
}

pub(crate) fn run_view_command(
    config: &config::AppConfig,
    id: &str,
) -> Result<EntryDetail, AppError> {
    with_store(config, |store| entry::view_entry(store, config, id))
}

pub(crate) fn run_mark_response(
    config: &config::AppConfig,
    command: &MarkCommand,
) -> Result<MarkResponse, AppError> {
    with_store(config, |store| {
        let updated = run_mark_command(store, config, command)?;
        store.bump_revision(current_epoch())?;
        Ok(MarkResponse {
            updated_entry_count: updated,
        })
    })
}

/// Executes sync command and streams plain progress lines to stdout.
pub(crate) fn run_sync_command_plain(
    config: &config::AppConfig,
) -> Result<output::PlainOutput, RunFailure> {
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
    Ok(output::PlainOutput::Sync(summary))
}

/// Executes sync command using the shared store path without progress rendering.
fn run_sync_with_store(
    config: &config::AppConfig,
    store: &mut db::sqlite::SqliteStore,
) -> Result<SyncSummary, AppError> {
    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
    let summary = sync::run_sync(store, config, &feeds_config)?;
    let now = current_epoch();
    store.bump_revision(now)?;
    store.update_sync(now, summary.status.as_str())?;
    Ok(summary)
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

fn with_store<T>(
    config: &config::AppConfig,
    f: impl FnOnce(&mut db::sqlite::SqliteStore) -> Result<T, AppError>,
) -> Result<T, AppError> {
    debug!(
        db_path = ?config.database.path,
        feeds_path = ?config.feeds.source,
        "loaded configuration"
    );
    let mut store = db::sqlite::SqliteStore::open(&config.database.path)?;
    store.migrate()?;
    f(&mut store)
}

fn parse_tag_list(raw: Option<&str>) -> Vec<String> {
    parse_tag_csv(raw)
}
