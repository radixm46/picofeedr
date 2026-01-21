//! SQLite adapter for the store.

use crate::db::{EntryContentInput, EntryInput, EntryInsertResult, FeedInput, FeedRow};
use crate::error::AppError;
use rusqlite::{Connection, params, params_from_iter};
use std::collections::{HashMap, HashSet};
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

    /// Fetches a feed ID by feed_key.
    pub fn find_feed_id(&self, feed_key: &str) -> Result<Option<i64>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM feeds WHERE feed_key = ?1")?;
        let mut rows = stmt.query(params![feed_key])?;
        if let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            return Ok(Some(id));
        }
        Ok(None)
    }

    /// Inserts an entry and returns its ID and insertion status.
    pub fn insert_entry(&self, entry: &EntryInput) -> Result<EntryInsertResult, AppError> {
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO entries (
                entry_key,
                feed_id,
                source_id,
                link,
                title,
                author,
                published_at,
                updated_at,
                first_seen_at,
                meta_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.entry_key,
                entry.feed_id,
                entry.source_id,
                entry.link,
                entry.title,
                entry.author,
                entry.published_at,
                entry.updated_at,
                entry.first_seen_at,
                entry.meta_json
            ],
        )? > 0;
        let entry_id: i64 = self.conn.query_row(
            "SELECT id FROM entries WHERE entry_key = ?1",
            params![entry.entry_key],
            |row| row.get(0),
        )?;
        Ok(EntryInsertResult { entry_id, inserted })
    }

    /// Inserts entry content for a newly inserted entry.
    pub fn insert_entry_content(
        &self,
        entry_id: i64,
        content: &EntryContentInput,
    ) -> Result<(), AppError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO entry_contents (entry_id, storage, ref, content_type, content)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                entry_id,
                content.storage,
                content.reference,
                content.content_type,
                content.content
            ],
        )?;
        Ok(())
    }

    /// Inserts tags for an entry.
    pub fn insert_entry_tags(&self, entry_id: i64, tags: &[String]) -> Result<(), AppError> {
        if tags.is_empty() {
            return Ok(());
        }
        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for tag in tags {
            if seen.insert(tag.clone()) {
                unique.push(tag.clone());
            }
        }
        for tag in &unique {
            self.conn.execute(
                "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
                params![tag],
            )?;
        }
        let placeholders = std::iter::repeat_n("?", unique.len())
            .collect::<Vec<_>>()
            .join(",");
        let query = format!("SELECT id, name FROM tags WHERE name IN ({placeholders})");
        let mut stmt = self.conn.prepare(&query)?;
        let mut rows = stmt.query(params_from_iter(unique.iter()))?;
        let mut tag_ids = HashMap::new();
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            tag_ids.insert(name, id);
        }
        for tag in &unique {
            let tag_id = tag_ids
                .get(tag)
                .ok_or_else(|| AppError::db(format!("Missing tag id for {tag}")))?;
            self.conn.execute(
                "INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?1, ?2)",
                params![entry_id, tag_id],
            )?;
        }
        Ok(())
    }
}
