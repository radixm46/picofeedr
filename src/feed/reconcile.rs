//! Feed reconciliation against configured feeds.

use crate::config::feeds::FeedsConfig;
use crate::db::sqlite::SqliteStore;
use crate::error::AppError;

/// Reconciles configured feeds into SQLite.
pub fn reconcile_feeds(store: &mut SqliteStore, config: &FeedsConfig) -> Result<(), AppError> {
    let tx = store.tx()?;
    tx.feed_write_repo().reconcile_feeds(config)?;
    tx.commit()?;
    Ok(())
}
