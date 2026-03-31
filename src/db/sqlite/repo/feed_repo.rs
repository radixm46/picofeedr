//! Feed repositories for SQLite-backed operations.

use crate::config::feeds::{FeedConfig, FeedsConfig};
use crate::db::sqlite::{feeds, tags};
use crate::db::{FeedInput, FeedRow};
use crate::error::AppError;
use crate::feed::feed_id_from_url;
use crate::tag::dedupe_tag_names;
use crate::time::current_epoch;
use rusqlite::Connection;
use std::collections::HashMap;

/// Read-only repository for feed query operations.
pub struct FeedReadRepo<'a> {
    conn: &'a Connection,
}

impl<'a> FeedReadRepo<'a> {
    /// Creates a read repository bound to one SQLite connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Returns all feeds stored in the database.
    pub fn list_feeds(&self) -> Result<Vec<FeedRow>, AppError> {
        feeds::list_feeds_with_conn(self.conn)
    }

    /// Resolves feed primary keys by feed ids.
    pub fn find_feed_pks_by_ids(
        &self,
        feed_ids: &[String],
    ) -> Result<HashMap<String, i64>, AppError> {
        feeds::find_feed_pks_by_ids_with_conn(self.conn, feed_ids)
    }
}

/// Write-oriented repository for feed and tag reconciliation operations.
pub struct FeedWriteRepo<'a> {
    conn: &'a Connection,
}

impl<'a> FeedWriteRepo<'a> {
    /// Creates a write repository bound to one SQLite transaction connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Upserts one feed row.
    pub fn upsert_feed(&self, feed: &FeedInput, now: i64) -> Result<(), AppError> {
        feeds::upsert_feed_with_conn(self.conn, feed, now)
    }

    /// Ensures one tag exists.
    pub fn ensure_tag(&self, name: &str) -> Result<(), AppError> {
        tags::ensure_tag_with_conn(self.conn, name)
    }

    /// Ensures feeds and tags from config exist in SQLite.
    pub fn reconcile_feeds(&self, config: &FeedsConfig, unread_tag: &str) -> Result<(), AppError> {
        let now = current_epoch();
        let mut all_tags = config.all_tags();
        for rule in &config.auto_tags {
            all_tags.extend(rule.add_tags.iter().cloned());
        }
        for feed in &config.feeds {
            for rule in &feed.auto_tags {
                all_tags.extend(rule.add_tags.iter().cloned());
            }
        }
        for tag in dedupe_tag_names(
            all_tags
                .into_iter()
                .chain(std::iter::once(unread_tag.to_string())),
        ) {
            self.ensure_tag(&tag)?;
        }
        for feed in &config.feeds {
            let input = feed_input(feed);
            self.upsert_feed(&input, now)?;
        }
        Ok(())
    }
}

/// Builds a FeedInput payload from a FeedConfig entry.
fn feed_input(feed: &FeedConfig) -> FeedInput {
    FeedInput {
        feed_id: feed_id_from_url(&feed.url),
        url: feed.url.clone(),
        title: feed.title.clone(),
        author: None,
        site_url: None,
        meta_json: None,
    }
}
