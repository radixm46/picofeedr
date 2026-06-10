//! Feed repositories for SQLite-backed operations.

use crate::config::feeds::{FeedConfig, FeedsConfig};
use crate::db::sqlite::feeds;
use crate::db::{FeedInput, FeedRow};
use crate::error::AppError;
use crate::feed::feed_id_from_url;
use crate::sync::model::FeedMetadata;
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

/// Write-oriented repository for feed catalog write operations.
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

    /// Ensures active feeds from config exist in SQLite.
    pub fn ensure_active_feeds(&self, config: &FeedsConfig) -> Result<(), AppError> {
        let now = current_epoch();
        for feed in config.active_feeds() {
            let input = feed_input(feed);
            feeds::upsert_feed_from_config_with_conn(self.conn, &input, now)?;
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
    fn ensure_active_feeds_preserves_existing_metadata_fields() {
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
            .ensure_active_feeds(&config)
            .expect("ensure active feeds");

        let feeds = list_feeds_with_conn(&conn).expect("list feeds");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].title.as_deref(), Some("Configured Title"));
        assert_eq!(feeds[0].author.as_deref(), Some("Stored Author"));
        assert_eq!(
            feeds[0].site_url.as_deref(),
            Some("https://example.com/site")
        );
    }

    #[test]
    fn ensure_active_feeds_does_not_ensure_tags_declared_for_active_feeds() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migrate");
        let temp = TempDir::new().expect("temp dir");
        let feeds_path = temp.path().join("feeds.yaml");
        std::fs::write(
            &feeds_path,
            r#"picofeedr:
  active:
    tags: [active-group]
    auto_tags:
      - title_contains: [active]
        add_tags: [active-auto]
    feeds:
      - url: "https://example.com/active.xml"
        title: Active
        tags: [active-feed]
  skipped:
    tags: [skipped-group]
    auto_tags:
      - title_contains: [skipped]
        add_tags: [skipped-auto]
    feeds:
      - url: "https://example.com/skipped.xml"
        title: Skipped
        tags: [skipped-feed]
        skip: true
"#,
        )
        .expect("write feeds yaml");
        let config = FeedsConfig::load(&feeds_path).expect("load feeds");

        FeedWriteRepo::new(&conn)
            .ensure_active_feeds(&config)
            .expect("ensure active feeds");

        let tag_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .expect("count tags");
        assert_eq!(tag_count, 0);
    }

    #[test]
    fn ensure_active_feeds_does_not_ensure_unread_or_auto_tags_without_active_feeds() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migrate");
        let temp = TempDir::new().expect("temp dir");
        let feeds_path = temp.path().join("feeds.yaml");
        std::fs::write(
            &feeds_path,
            r#"picofeedr:
  auto_tags:
    - title_contains: [anything]
      add_tags: [root-auto]
  skipped:
    tags: [skipped-group]
    feeds:
      - url: "https://example.com/skipped.xml"
        title: Skipped
        tags: [skipped-feed]
        skip: true
"#,
        )
        .expect("write feeds yaml");
        let config = FeedsConfig::load(&feeds_path).expect("load feeds");

        FeedWriteRepo::new(&conn)
            .ensure_active_feeds(&config)
            .expect("ensure active feeds");

        let tag_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .expect("count tags");
        assert_eq!(tag_count, 0);
    }
}
