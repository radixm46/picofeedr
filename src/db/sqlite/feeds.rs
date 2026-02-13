//! Feed queries for SQLite store.

use crate::db::{FeedInput, FeedRow};
use crate::error::AppError;
use rusqlite::{Connection, params, params_from_iter};
use std::collections::HashMap;

/// Returns all feeds stored in the database.
pub(crate) fn list_feeds_with_conn(conn: &Connection) -> Result<Vec<FeedRow>, AppError> {
    let mut stmt =
        conn.prepare("SELECT id, feed_key, url, title, author, site_url FROM feeds ORDER BY id")?;
    let feeds = stmt
        .query_map([], |row| {
            Ok(FeedRow {
                id: row.get(0)?,
                feed_key: row.get(1)?,
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
pub(crate) fn upsert_feed_with_conn(
    conn: &Connection,
    feed: &FeedInput,
    now: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO feeds (feed_key, url, title, author, site_url, meta_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(feed_key) DO UPDATE SET
            url = excluded.url,
            title = excluded.title,
            author = excluded.author,
            site_url = excluded.site_url,
            meta_json = excluded.meta_json,
            updated_at = excluded.updated_at",
        params![
            feed.feed_key,
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

/// Fetches feed IDs by feed_key using chunked IN queries.
pub(crate) fn find_feed_ids_with_conn(
    conn: &Connection,
    feed_keys: &[String],
) -> Result<HashMap<String, i64>, AppError> {
    const FEED_KEY_CHUNK_SIZE: usize = 500;

    if feed_keys.is_empty() {
        return Ok(HashMap::new());
    }
    let mut ids = HashMap::new();
    for chunk in feed_keys.chunks(FEED_KEY_CHUNK_SIZE) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("SELECT feed_key, id FROM feeds WHERE feed_key IN ({placeholders})");
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
        while let Some(row) = rows.next()? {
            let feed_key: String = row.get(0)?;
            let id: i64 = row.get(1)?;
            ids.insert(feed_key, id);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::{find_feed_ids_with_conn, upsert_feed_with_conn};
    use crate::db::FeedInput;
    use rusqlite::Connection;

    /// Resolves IDs for existing keys and skips missing keys.
    #[test]
    fn find_feed_ids_returns_existing_keys_only() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        crate::db::migrate::migrate(&conn).expect("migrate");
        upsert_feed_with_conn(
            &conn,
            &FeedInput {
                feed_key: "feed-a".to_string(),
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
                feed_key: "feed-b".to_string(),
                url: "https://example.com/b".to_string(),
                title: Some("B".to_string()),
                author: None,
                site_url: None,
                meta_json: None,
            },
            1,
        )
        .expect("upsert feed b");

        let keys = vec![
            "feed-a".to_string(),
            "feed-missing".to_string(),
            "feed-b".to_string(),
            "feed-a".to_string(),
        ];
        let ids = find_feed_ids_with_conn(&conn, &keys).expect("find ids");
        assert_eq!(ids.len(), 2);
        assert!(ids.contains_key("feed-a"));
        assert!(ids.contains_key("feed-b"));
    }
}
