//! Feed DAO for SQLite store.
//!
//! This module intentionally stays at single-statement query execution level.
//! Multi-step workflows live in the store and transaction wrappers.

use crate::db::sqlite::query::{feeds as q, sql_placeholders};
use crate::db::{FeedInput, FeedRow};
use crate::error::AppError;
use rusqlite::{Connection, params, params_from_iter};
use std::collections::HashMap;

/// Returns all feeds stored in the database.
pub(crate) fn list_feeds_with_conn(conn: &Connection) -> Result<Vec<FeedRow>, AppError> {
    let mut stmt = conn.prepare(q::SELECT_FEEDS)?;
    let feeds = stmt
        .query_map([], |row| {
            Ok(FeedRow {
                feed_pk: row.get(0)?,
                feed_id: row.get(1)?,
                url: row.get(2)?,
                title: row.get(3)?,
                author: row.get(4)?,
                site_url: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(feeds)
}

/// Inserts or updates a feed row using a provided connection.
#[cfg(test)]
pub(crate) fn upsert_feed_with_conn(
    conn: &Connection,
    feed: &FeedInput,
    now: i64,
) -> Result<(), AppError> {
    conn.execute(
        q::UPSERT_FEED,
        params![
            feed.feed_id,
            feed.url,
            feed.title,
            feed.author,
            feed.site_url,
            feed.meta_json,
            now,
            now
        ],
    )?;
    Ok(())
}

/// Inserts or updates config-owned feed fields without overwriting observed metadata.
pub(crate) fn upsert_feed_from_config_with_conn(
    conn: &Connection,
    feed: &FeedInput,
    now: i64,
) -> Result<(), AppError> {
    conn.execute(
        q::UPSERT_FEED_FROM_CONFIG,
        params![
            feed.feed_id,
            feed.url,
            feed.title,
            feed.author,
            feed.site_url,
            feed.meta_json,
            now,
            now
        ],
    )?;
    Ok(())
}

/// Refreshes persisted feed metadata with non-empty observed values.
pub(crate) fn refresh_feed_metadata_with_conn(
    conn: &Connection,
    feed_pk: i64,
    title: Option<&str>,
    author: Option<&str>,
    site_url: Option<&str>,
    now: i64,
) -> Result<(), AppError> {
    conn.execute(
        q::UPDATE_FEED_METADATA,
        params![title, author, site_url, now, feed_pk],
    )?;
    Ok(())
}

/// Fetches feed primary keys by feed_id using chunked IN queries.
pub(crate) fn find_feed_pks_by_ids_with_conn(
    conn: &Connection,
    feed_ids: &[String],
) -> Result<HashMap<String, i64>, AppError> {
    const FEED_ID_CHUNK_SIZE: usize = 500;

    if feed_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut feed_pks_by_id = HashMap::new();
    for chunk in feed_ids.chunks(FEED_ID_CHUNK_SIZE) {
        let placeholders = sql_placeholders(chunk.len());
        let sql = q::select_feed_pks_by_ids(&placeholders);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
        while let Some(row) = rows.next()? {
            let feed_id: String = row.get(0)?;
            let feed_pk: i64 = row.get(1)?;
            feed_pks_by_id.insert(feed_id, feed_pk);
        }
    }
    Ok(feed_pks_by_id)
}

#[cfg(test)]
mod tests {
    use super::{
        find_feed_pks_by_ids_with_conn, list_feeds_with_conn, upsert_feed_from_config_with_conn,
        upsert_feed_with_conn,
    };
    use crate::db::FeedInput;
    use rusqlite::Connection;

    #[test]
    fn upsert_feed_from_config_preserves_existing_metadata_fields() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        crate::db::migrate::migrate(&conn).expect("migrate");
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

        upsert_feed_with_conn(&conn, &existing, 1).expect("seed feed");
        upsert_feed_from_config_with_conn(&conn, &configured, 2).expect("ensure feed");

        let feeds = list_feeds_with_conn(&conn).expect("list feeds");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].title.as_deref(), Some("Configured Title"));
        assert_eq!(feeds[0].author.as_deref(), Some("Stored Author"));
        assert_eq!(
            feeds[0].site_url.as_deref(),
            Some("https://example.com/site")
        );
    }

    /// Resolves feed primary keys for existing feed ids and skips missing ids.
    #[test]
    fn find_feed_pks_by_ids_returns_existing_ids_only() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        crate::db::migrate::migrate(&conn).expect("migrate");
        upsert_feed_with_conn(
            &conn,
            &FeedInput {
                feed_id: "feed-a".to_string(),
                url: "https://example.com/a".to_string(),
                title: Some("A".to_string()),
                author: None,
                site_url: None,
                meta_json: None,
            },
            1,
        )
        .expect("upsert feed a");
        upsert_feed_with_conn(
            &conn,
            &FeedInput {
                feed_id: "feed-b".to_string(),
                url: "https://example.com/b".to_string(),
                title: Some("B".to_string()),
                author: None,
                site_url: None,
                meta_json: None,
            },
            1,
        )
        .expect("upsert feed b");

        let ids = vec![
            "feed-a".to_string(),
            "feed-missing".to_string(),
            "feed-b".to_string(),
            "feed-a".to_string(),
        ];
        let feed_pks = find_feed_pks_by_ids_with_conn(&conn, &ids).expect("find feed pks");
        assert_eq!(feed_pks.len(), 2);
        assert!(feed_pks.contains_key("feed-a"));
        assert!(feed_pks.contains_key("feed-b"));
    }
}
