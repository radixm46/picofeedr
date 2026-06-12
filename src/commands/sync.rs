use super::store::with_store;
use crate::{RunFailure, output};
use picofeedr::config;
use picofeedr::current_epoch;
use picofeedr::db;
use picofeedr::error::AppError;
use picofeedr::sync::{self, SyncSummary};
use std::io::{self, Write};
use tracing::debug;

pub(crate) fn run_sync_command(config: &config::AppConfig) -> Result<SyncSummary, AppError> {
    with_store(config, |store| run_sync_with_store(config, store, None))
}

/// Executes sync command and streams plain progress lines to stdout.
pub(crate) fn run_sync_command_plain(
    config: &config::AppConfig,
) -> Result<SyncSummary, RunFailure> {
    debug!(
        db_path = ?config.database.path,
        feeds_path = ?config.feeds.source,
        "loaded configuration"
    );
    let mut store = db::sqlite::SqliteStore::open(&config.database.path)?;
    store.migrate()?;
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

    let summary = run_sync_with_store(config, &mut store, Some(&mut on_progress))?;
    if let Some(error) = write_error {
        return Err(RunFailure::Io(error));
    }
    Ok(summary)
}

/// Executes sync command using the shared store path without progress rendering.
fn run_sync_with_store(
    config: &config::AppConfig,
    store: &mut db::sqlite::SqliteStore,
    on_progress: Option<&mut dyn FnMut(sync::SyncProgressEvent)>,
) -> Result<SyncSummary, AppError> {
    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
    feeds_config.ensure_valid_for_runtime()?;
    let summary = sync::run_sync_with_progress(store, config, &feeds_config, on_progress)?;
    let now = current_epoch();
    store.bump_revision(now)?;
    store.update_sync(now, summary.status.as_str())?;
    Ok(summary)
}
