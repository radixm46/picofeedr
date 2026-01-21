//! SQLite adapter for the store.

mod entries;
mod feeds;
mod tags;

use crate::db::FeedRow;
use crate::error::AppError;
use rusqlite::Connection;
use std::path::Path;
use std::time::Duration;

pub(crate) use entries::{
    insert_entry_content_with_conn, insert_entry_tags_with_conn, insert_entry_with_conn,
};
pub(crate) use feeds::{find_feed_id_with_conn, upsert_feed_with_conn};
pub(crate) use tags::ensure_tag_with_conn;

/// SQLite store wrapper.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Opens a SQLite database at the provided path.
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        // NOTE: Consider making busy_timeout configurable once needed.
        Ok(Self { conn })
    }

    /// Begins a transaction for grouped writes.
    pub fn transaction(&mut self) -> Result<rusqlite::Transaction<'_>, AppError> {
        Ok(self.conn.transaction()?)
    }

    /// Exposes the underlying connection for internal helpers.
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Applies schema migrations.
    pub fn migrate(&self) -> Result<(), AppError> {
        crate::db::migrate::migrate(&self.conn)
    }

    /// Returns all feeds stored in the database.
    pub fn list_feeds(&self) -> Result<Vec<FeedRow>, AppError> {
        feeds::list_feeds_with_conn(&self.conn)
    }

    /// Lists all tags ordered by name.
    pub fn list_tags(&self) -> Result<Vec<String>, AppError> {
        tags::list_tags_with_conn(&self.conn)
    }
}
