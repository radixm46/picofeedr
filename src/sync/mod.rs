//! Feed synchronization and ingestion.

mod autotag;
pub(crate) mod content;
mod fetch;
pub(crate) mod model;
mod normalize;

use crate::config::AppConfig;
use crate::config::feeds::FeedsConfig;
use crate::db::sqlite::SqliteStore;
use crate::error::AppError;
use crate::feed::feed_id_from_url;
use std::sync::Arc;
use std::time::Instant;

pub use model::{SyncStatus, SyncSummary};

use autotag::compile_auto_tags;
use fetch::fetch_parallel;
use model::SyncTarget;

/// Runs a sync for all feeds in config.
pub fn run_sync(
    store: &mut SqliteStore,
    config: &AppConfig,
    feeds_config: &FeedsConfig,
) -> Result<SyncSummary, AppError> {
    let start = Instant::now();
    let compiled_rules = Arc::new(compile_auto_tags(&feeds_config.auto_tags)?);
    let targets = build_sync_targets(feeds_config)?;
    let (results, errors) = fetch_parallel(&targets, config, Arc::clone(&compiled_rules))?;

    let tx = store.tx()?;
    tx.feed_write_repo()
        .reconcile_feeds(feeds_config, &config.unread_tag)?;
    let new_entry_count = tx.sync_write_repo().ingest_results(config, results)?;
    tx.commit()?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let failed_feed_count = errors.len();
    let status = if failed_feed_count > 0 && failed_feed_count == targets.len() {
        SyncStatus::Failed
    } else if failed_feed_count > 0 {
        SyncStatus::PartialFailed
    } else {
        SyncStatus::Completed
    };
    Ok(SyncSummary {
        status,
        fetched_feed_count: targets.len(),
        failed_feed_count,
        new_entry_count,
        duration_ms,
        errors,
    })
}

/// Builds sync targets from feeds configuration.
fn build_sync_targets(feeds_config: &FeedsConfig) -> Result<Vec<SyncTarget>, AppError> {
    let mut targets = Vec::new();
    for feed in &feeds_config.feeds {
        let feed_id = feed_id_from_url(&feed.url);
        targets.push(SyncTarget {
            feed_id,
            url: feed.url.clone(),
            tags: feed.tags.clone(),
        });
    }
    Ok(targets)
}
