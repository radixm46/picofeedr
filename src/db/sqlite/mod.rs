//! SQLite adapter for the store.

mod entries;
mod feeds;
mod meta;
pub(crate) mod query;
pub mod repo;
pub(crate) mod schema;
mod tags;

use crate::db::FeedRow;
use crate::db::sqlite::repo::{EntryRepo, FeedRepo, SyncRepo};
use crate::error::AppError;
use rusqlite::Connection;
use std::path::Path;
use std::time::Duration;

pub(crate) use meta::SystemMeta;

/// Transaction wrapper exposing repository APIs.
pub struct Tx<'conn> {
    tx: rusqlite::Transaction<'conn>,
}

impl<'conn> Tx<'conn> {
    /// Creates a transaction wrapper.
    fn new(tx: rusqlite::Transaction<'conn>) -> Self {
        Self { tx }
    }

    /// Returns an entry repository scoped to this transaction.
    pub fn entry_repo(&self) -> EntryRepo<'_> {
        EntryRepo::new(&self.tx)
    }

    /// Returns a feed repository scoped to this transaction.
    pub fn feed_repo(&self) -> FeedRepo<'_> {
        FeedRepo::new(&self.tx)
    }

    /// Returns a sync repository scoped to this transaction.
    pub fn sync_repo(&self) -> SyncRepo<'_> {
        SyncRepo::new(&self.tx)
    }

    /// Commits the transaction.
    pub fn commit(self) -> Result<(), AppError> {
        Ok(self.tx.commit()?)
    }
}

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

    /// Begins a wrapped transaction for repository-based operations.
    pub fn tx(&mut self) -> Result<Tx<'_>, AppError> {
        Ok(Tx::new(self.conn.transaction()?))
    }

    /// Returns an entry repository bound to the store connection.
    pub fn entry_repo(&self) -> EntryRepo<'_> {
        EntryRepo::new(&self.conn)
    }

    /// Returns a feed repository bound to the store connection.
    pub fn feed_repo(&self) -> FeedRepo<'_> {
        FeedRepo::new(&self.conn)
    }

    /// Returns a sync repository bound to the store connection.
    pub fn sync_repo(&self) -> SyncRepo<'_> {
        SyncRepo::new(&self.conn)
    }

    /// Applies schema migrations.
    pub fn migrate(&self) -> Result<(), AppError> {
        crate::db::migrate::migrate(&self.conn)
    }

    /// Returns all feeds stored in the database.
    pub fn list_feeds(&self) -> Result<Vec<FeedRow>, AppError> {
        self.feed_repo().list_feeds()
    }

    /// Lists all tags ordered by name.
    pub fn list_tags(&self) -> Result<Vec<String>, AppError> {
        tags::list_tags_with_conn(&self.conn)
    }

    /// Returns database status metadata stored in `es_meta`.
    pub fn read_system_meta(&self) -> Result<SystemMeta, AppError> {
        self.sync_repo().read_system_meta()
    }

    /// Increments database revision and updates write timestamp.
    pub fn bump_revision(&self, now: i64) -> Result<SystemMeta, AppError> {
        self.sync_repo().bump_revision(now)
    }

    /// Updates the latest sync timestamp and status metadata.
    pub fn update_sync(&self, now: i64, status: &str) -> Result<SystemMeta, AppError> {
        self.sync_repo().update_sync(now, status)
    }
}
