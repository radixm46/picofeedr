//! Feed reconciliation against configured feeds.

use crate::config::feeds::FeedsConfig;
use crate::db::sqlite::SqliteStore;
use crate::error::AppError;

/// Reconciles configured feeds and known tags into SQLite.
pub fn reconcile_feeds(
    store: &mut SqliteStore,
    config: &FeedsConfig,
    unread_tag: Option<&str>,
) -> Result<(), AppError> {
    let tx = store.tx()?;
    tx.feed_write_repo().reconcile_feeds(config, unread_tag)?;
    tx.commit()?;
    Ok(())
}
