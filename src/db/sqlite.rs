//! SQLite adapter for the store.

use crate::db::{FeedInput, FeedRow};
use crate::error::AppError;
use rusqlite::{Connection, params};
use std::path::Path;

/// SQLite store wrapper.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Opens a SQLite database at the provided path.
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self { conn })
    }

    /// Applies schema migrations.
    pub fn migrate(&self) -> Result<(), AppError> {
        crate::db::migrate::migrate(&self.conn)
    }

    /// Returns all feeds stored in the database.
    pub fn list_feeds(&self) -> Result<Vec<FeedRow>, AppError> {
        let mut stmt = self.conn.prepare(
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

    /// Inserts or updates a feed row in the database.
    pub fn upsert_feed(&self, feed: &FeedInput, now: i64) -> Result<(), AppError> {
        self.conn.execute(
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

    /// Inserts a tag if it does not exist.
    pub fn ensure_tag(&self, name: &str) -> Result<(), AppError> {
        self.conn.execute(
            "INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
            params![name],
        )?;
        Ok(())
    }

    /// Lists all tags ordered by name.
    pub fn list_tags(&self) -> Result<Vec<String>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM tags ORDER BY name ASC")?;
        let tags = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(tags)
    }
}
