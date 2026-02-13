//! Feed reconciliation against configured feeds.

use crate::config::feeds::FeedsConfig;
use crate::db::sqlite::SqliteStore;
use crate::error::AppError;

/// Reconciles configured feeds and known tags into SQLite.
pub fn reconcile_feeds(
    store: &SqliteStore,
    config: &FeedsConfig,
    unread_tag: &str,
) -> Result<(), AppError> {
    store.feed_repo().reconcile_feeds(config, unread_tag)
}
