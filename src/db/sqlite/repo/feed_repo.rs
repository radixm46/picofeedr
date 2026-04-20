//! Feed repositories for SQLite-backed operations.

use crate::config::feeds::{FeedConfig, FeedsConfig};
use crate::db::sqlite::{feeds, tags};
use crate::db::{FeedInput, FeedRow};
use crate::error::AppError;
use crate::feed::feed_id_from_url;
use crate::sync::model::FeedMetadata;
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

    /// Refreshes non-empty feed metadata on an existing feed row.
    pub(crate) fn refresh_feed_metadata(
        &self,
        feed_pk: i64,
        metadata: &FeedMetadata,
        now: i64,
    ) -> Result<(), AppError> {
        if !metadata.has_values() {
            return Ok(());
        }
        feeds::refresh_feed_metadata_with_conn(
            self.conn,
            feed_pk,
            metadata.title.as_deref(),
            metadata.author.as_deref(),
            metadata.site_url.as_deref(),
            now,
        )
    }

    /// Ensures one tag exists.
    pub fn ensure_tag(&self, name: &str) -> Result<(), AppError> {
        tags::ensure_tag_with_conn(self.conn, name)
    }

    /// Ensures feeds and tags from config exist in SQLite.
    pub fn reconcile_feeds(
        &self,
        config: &FeedsConfig,
        unread_tag: Option<&str>,
    ) -> Result<(), AppError> {
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
        let tag_iter = all_tags
            .into_iter()
            .chain(unread_tag.into_iter().map(str::to_string));
        for tag in dedupe_tag_names(tag_iter) {
            self.ensure_tag(&tag)?;
        }
        for feed in &config.feeds {
            let input = feed_input(feed);
            feeds::reconcile_feed_with_conn(self.conn, &input, now)?;
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

#[cfg(test)]
mod tests {
    use super::FeedWriteRepo;
    use crate::config::feeds::FeedsConfig;
    use crate::db::FeedInput;
    use crate::db::migrate::migrate;
    use crate::db::sqlite::feeds::list_feeds_with_conn;
    use crate::db::sqlite::repo::feed_repo::feed_input;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn write_feeds_yaml(temp: &TempDir, url: &str, title: &str) -> std::path::PathBuf {
        let path = temp.path().join("feeds.yaml");
        std::fs::write(
            &path,
            format!(
                "picofeedr:\n  feeds:\n    - url: \"{url}\"\n      title: \"{title}\"\n      tags: [tech]\n"
            ),
        )
        .expect("write feeds yaml");
        path
    }

    #[test]
    fn reconcile_feeds_preserves_existing_metadata_fields() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migrate");
        let temp = TempDir::new().expect("temp dir");
        let feed_url = "https://example.com/feed.xml";
        let feeds_path = write_feeds_yaml(&temp, feed_url, "Configured Title");
        let config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let existing = FeedInput {
            author: Some("Stored Author".to_string()),
            site_url: Some("https://example.com/site".to_string()),
            ..feed_input(&config.feeds[0])
        };

        crate::db::sqlite::feeds::upsert_feed_with_conn(&conn, &existing, 1).expect("seed feed");

        FeedWriteRepo::new(&conn)
            .reconcile_feeds(&config, Some("unread"))
            .expect("reconcile feeds");

        let feeds = list_feeds_with_conn(&conn).expect("list feeds");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].title.as_deref(), Some("Configured Title"));
        assert_eq!(feeds[0].author.as_deref(), Some("Stored Author"));
        assert_eq!(
            feeds[0].site_url.as_deref(),
            Some("https://example.com/site")
        );
    }
}
