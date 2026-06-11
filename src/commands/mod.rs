//! Command orchestration for CLI runtime.

mod list;
mod mark;
mod store;
mod sync;

use crate::{CommandOutcome, CommandRun, RunFailure, version_response};
use picofeedr::TagManager;
use picofeedr::cli::{Command, OutputFormat};
use picofeedr::config;
use picofeedr::entry::{self, EntryDetail};
use picofeedr::error::AppError;
use picofeedr::feed::{self, FeedListResponse};
use picofeedr::response::TagListResponse;
use picofeedr::status::StatusResponse;
use std::process::ExitCode;
use store::with_store;

pub(crate) use list::run_list_command;
pub(crate) use mark::run_mark_command;
pub(crate) use sync::{run_sync_command, run_sync_command_plain};

/// Executes a CLI command and returns its typed outcome and exit code.
///
/// This is the single dispatch over [`Command`]; output-format specifics are
/// limited to `sync`, whose plain mode streams progress during execution.
pub(crate) fn run_command(
    command: &Command,
    output: OutputFormat,
    config: &config::AppConfig,
) -> Result<CommandRun, RunFailure> {
    let outcome = match command {
        Command::Version => CommandOutcome::Version(version_response()),
        Command::Tags => CommandOutcome::Tags(run_tags_command(config)?),
        Command::Status => CommandOutcome::Status(run_status_command(config)?),
        Command::Feeds { id } => CommandOutcome::Feeds {
            feeds: run_feeds_command(config)?,
            include_id: *id,
        },
        Command::Sync { check: true } => {
            let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
            let report = feeds_config.validate();
            let exit_code = if report.valid {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            };
            return Ok(CommandRun::with_exit_code(
                CommandOutcome::SyncCheck(report),
                exit_code,
            ));
        }
        Command::Sync { check: false } => {
            let summary = match output {
                OutputFormat::Json => run_sync_command(config)?,
                OutputFormat::Plain => run_sync_command_plain(config)?,
            };
            CommandOutcome::Sync(summary)
        }
        Command::List {
            query,
            sort,
            limit,
            cursor,
            id,
        } => CommandOutcome::List {
            list: run_list_command(config, query.as_deref(), *sort, *limit, cursor.as_deref())?,
            include_id: *id,
        },
        Command::View { id } => CommandOutcome::View(run_view_command(config, id)?),
        Command::Mark { command } => CommandOutcome::Mark(run_mark_command(config, command)?),
    };

    Ok(CommandRun::success(outcome))
}

pub(crate) fn run_tags_command(config: &config::AppConfig) -> Result<TagListResponse, AppError> {
    with_store(config, |store| {
        let tag_manager = TagManager::new(store);
        let tags = tag_manager.list_tags()?;
        Ok(TagListResponse { tags })
    })
}

pub(crate) fn run_status_command(config: &config::AppConfig) -> Result<StatusResponse, AppError> {
    with_store(config, |store| {
        let db_schema_version = store.schema_version()?;
        let meta = store.read_system_meta()?;
        Ok(StatusResponse::from_system_meta(
            &meta,
            db_schema_version,
            env!("CARGO_PKG_VERSION"),
        ))
    })
}

pub(crate) fn run_feeds_command(config: &config::AppConfig) -> Result<FeedListResponse, AppError> {
    with_store(config, |store| {
        let db_feeds = store.list_feeds()?;
        Ok(feed::build_feed_list_response(&db_feeds))
    })
}

pub(crate) fn run_view_command(
    config: &config::AppConfig,
    id: &str,
) -> Result<EntryDetail, AppError> {
    with_store(config, |store| entry::view_entry(store, config, id))
}
