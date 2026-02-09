//! Feed reconciliation logic.

use crate::config::feeds::{FeedConfig, FeedsConfig};
use crate::db::FeedInput;
use crate::db::sqlite::{SqliteStore, ensure_tag_with_conn, upsert_feed_with_conn};
use crate::error::AppError;
use crate::time::current_epoch;
use rusqlite::Connection;
use std::collections::HashSet;

use super::identity::feed_key_from_url;

/// Reconciles feeds.yaml with the database and ensures tag dictionary.
pub fn reconcile_feeds(
    store: &SqliteStore,
    config: &FeedsConfig,
    unread_tag: &str,
) -> Result<(), AppError> {
    reconcile_feeds_with_conn(store.connection(), config, unread_tag)
}

/// Reconciles feeds using a connection reference.
pub fn reconcile_feeds_with_conn(
    conn: &Connection,
    config: &FeedsConfig,
    unread_tag: &str,
) -> Result<(), AppError> {
    let now = current_epoch();
    let mut all_tags = config.all_tags();
    for rule in &config.auto_tags {
        all_tags.extend(rule.add_tags.iter().cloned());
    }
    all_tags.push(unread_tag.to_string());
    let mut seen = HashSet::new();
    for tag in all_tags {
        if seen.insert(tag.clone()) {
            ensure_tag_with_conn(conn, &tag)?;
        }
    }
    for feed in &config.feeds {
        let input = feed_input(feed);
        upsert_feed_with_conn(conn, &input, now)?;
    }
    Ok(())
}

/// Builds a FeedInput payload from a FeedConfig entry.
fn feed_input(feed: &FeedConfig) -> FeedInput {
    FeedInput {
        feed_key: feed_key_from_url(&feed.url),
        url: feed.url.clone(),
        title: feed.title.clone(),
        author: None,
        site_url: None,
        meta_json: None,
    }
}
