//! Feed queries for SQLite store.

use crate::db::{FeedInput, FeedRow};
use crate::error::AppError;
use rusqlite::{Connection, params};

/// Returns all feeds stored in the database.
pub(crate) fn list_feeds_with_conn(conn: &Connection) -> Result<Vec<FeedRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_key, url, title, author, site_url, meta_json FROM feeds ORDER BY id",
    )?;
    let feeds = stmt
        .query_map([], |row| {
            Ok(FeedRow {
                id: row.get(0)?,
                feed_key: row.get(1)?,
                url: row.get(2)?,
                title: row.get(3)?,
                author: row.get(4)?,
                site_url: row.get(5)?,
                meta_json: row.get(6)?,
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

/// Fetches a feed ID by feed_key using a provided connection.
pub(crate) fn find_feed_id_with_conn(
    conn: &Connection,
    feed_key: &str,
) -> Result<Option<i64>, AppError> {
    let mut stmt = conn.prepare("SELECT id FROM feeds WHERE feed_key = ?1")?;
    let mut rows = stmt.query(params![feed_key])?;
    if let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        return Ok(Some(id));
    }
    Ok(None)
}
