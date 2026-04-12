//! Command orchestration for CLI runtime.

mod list;
mod mark;
mod store;
mod sync;

use crate::{RunFailure, output};
use picofeedr::TagManager;
use picofeedr::cli::{Cli, Command};
use picofeedr::config;
use picofeedr::current_epoch;
use picofeedr::db;
use picofeedr::entry::{self, EntryDetail};
use picofeedr::error::AppError;
use picofeedr::feed::{self, FeedListResponse};
use picofeedr::response::TagListResponse;
use picofeedr::status::StatusResponse;
use store::with_store;

pub(crate) use list::run_list_command;
pub(crate) use mark::run_mark_command;
pub(crate) use sync::{run_sync_command, run_sync_command_plain};

pub(crate) fn run_plain_command(
    cli: &Cli,
    config: &config::AppConfig,
) -> Result<output::PlainOutput, RunFailure> {
    match &cli.command {
        Command::Tags => Ok(output::PlainOutput::Tags(run_tags_command(config)?.tags)),
        Command::Status => Ok(output::PlainOutput::Status(run_status_command(config)?)),
        Command::Feeds { id } => Ok(output::PlainOutput::Feeds {
            feeds: run_feeds_command(config)?,
            include_id: *id,
        }),
        Command::Sync { .. } => run_sync_command_plain(config),
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
        Command::Mark { command } => Ok(output::PlainOutput::Mark(run_mark_command(
            config, command,
        )?)),
        Command::Version => unreachable!("handled in main"),
    }
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
        feeds_config.ensure_valid_for_runtime()?;
        feed::reconcile_feeds(store, &feeds_config, &config.unread_tag)?;
        let db_feeds = store.list_feeds()?;
        let feeds = feed::build_feed_list_response(&feeds_config, &db_feeds);
        store.bump_revision(current_epoch())?;
        Ok(feeds)
    })
}

pub(crate) fn run_view_command(
    config: &config::AppConfig,
    id: &str,
) -> Result<EntryDetail, AppError> {
    with_store(config, |store| entry::view_entry(store, config, id))
}
