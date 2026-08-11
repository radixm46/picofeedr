//! Feed repositories for SQLite-backed operations.

use crate::db::sqlite::feeds;
use crate::db::{FeedInput, FeedMetadataInput, FeedRow};
use crate::error::AppError;
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

    /// Refreshes observed feed metadata on an existing feed row.
    pub(crate) fn refresh_feed_metadata(
        &self,
        feed_pk: i64,
        metadata: &FeedMetadataInput,
        now: i64,
    ) -> Result<(), AppError> {
        feeds::refresh_feed_metadata_with_conn(
            self.conn,
            feed_pk,
            metadata.title.as_deref(),
            metadata.author.as_deref(),
            metadata.site_url.as_deref(),
            now,
        )
    }

    /// Ensures configured feeds exist in SQLite.
    pub(crate) fn ensure_feeds(&self, feed_inputs: &[FeedInput], now: i64) -> Result<(), AppError> {
        for feed in feed_inputs {
            feeds::upsert_feed_from_config_with_conn(self.conn, feed, now)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::FeedWriteRepo;
    use crate::db::FeedInput;
    use crate::db::migrate::migrate;
    use crate::db::sqlite::feeds::list_feeds_with_conn;
    use rusqlite::Connection;

    #[test]
    fn ensure_feeds_preserves_existing_metadata_fields() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migrate");
        let feed_url = "https://example.com/feed.xml";
        let existing = FeedInput {
            feed_id: "feed-id".to_string(),
            url: feed_url.to_string(),
            title: Some("Configured Title".to_string()),
            author: Some("Stored Author".to_string()),
            site_url: Some("https://example.com/site".to_string()),
            meta_json: None,
        };
        let configured = FeedInput {
            feed_id: existing.feed_id.clone(),
            url: existing.url.clone(),
            title: existing.title.clone(),
            author: None,
            site_url: None,
            meta_json: None,
        };

        crate::db::sqlite::feeds::upsert_feed_with_conn(&conn, &existing, 1).expect("seed feed");

        FeedWriteRepo::new(&conn)
            .ensure_feeds(std::slice::from_ref(&configured), 2)
            .expect("ensure feeds");

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
