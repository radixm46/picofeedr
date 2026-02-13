//! SQLite adapter for the store.

mod entries;
mod feeds;
mod meta;
pub(crate) mod query;
pub mod repo;
pub(crate) mod schema;
mod tags;

use crate::db::FeedRow;
use crate::db::sqlite::repo::{
    EntryReadRepo, EntryWriteRepo, FeedReadRepo, FeedWriteRepo, SyncReadRepo, SyncWriteRepo,
};
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

    /// Returns a read-only entry repository scoped to this transaction.
    pub fn entry_read_repo(&self) -> EntryReadRepo<'_> {
        EntryReadRepo::new(&self.tx)
    }

    /// Returns a write entry repository scoped to this transaction.
    pub fn entry_write_repo(&self) -> EntryWriteRepo<'_> {
        EntryWriteRepo::new(&self.tx)
    }

    /// Returns a read-only feed repository scoped to this transaction.
    pub fn feed_read_repo(&self) -> FeedReadRepo<'_> {
        FeedReadRepo::new(&self.tx)
    }

    /// Returns a write feed repository scoped to this transaction.
    pub fn feed_write_repo(&self) -> FeedWriteRepo<'_> {
        FeedWriteRepo::new(&self.tx)
    }

    /// Returns a read-only sync repository scoped to this transaction.
    pub fn sync_read_repo(&self) -> SyncReadRepo<'_> {
        SyncReadRepo::new(&self.tx)
    }

    /// Returns a write sync repository scoped to this transaction.
    pub fn sync_write_repo(&self) -> SyncWriteRepo<'_> {
        SyncWriteRepo::new(&self.tx)
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
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        // NOTE: Consider making busy_timeout configurable once needed.
        Ok(Self { conn })
    }

    /// Begins a raw transaction.
    ///
    /// This is kept for internal compatibility. Prefer `tx()` in new code.
    pub fn transaction(&mut self) -> Result<rusqlite::Transaction<'_>, AppError> {
        Ok(self.conn.transaction()?)
    }

    /// Begins a wrapped transaction for repository-based write operations.
    pub fn tx(&mut self) -> Result<Tx<'_>, AppError> {
        Ok(Tx::new(self.conn.transaction()?))
    }

    /// Returns a read-only entry repository bound to the store connection.
    pub fn entry_read_repo(&self) -> EntryReadRepo<'_> {
        EntryReadRepo::new(&self.conn)
    }

    /// Returns a read-only feed repository bound to the store connection.
    pub fn feed_read_repo(&self) -> FeedReadRepo<'_> {
        FeedReadRepo::new(&self.conn)
    }

    /// Returns a read-only sync repository bound to the store connection.
    pub fn sync_read_repo(&self) -> SyncReadRepo<'_> {
        SyncReadRepo::new(&self.conn)
    }

    /// Applies schema migrations.
    pub fn migrate(&self) -> Result<(), AppError> {
        crate::db::migrate::migrate(&self.conn)
    }

    /// Returns all feeds stored in the database.
    pub fn list_feeds(&self) -> Result<Vec<FeedRow>, AppError> {
        self.feed_read_repo().list_feeds()
    }

    /// Lists all tags ordered by name.
    pub fn list_tags(&self) -> Result<Vec<String>, AppError> {
        tags::list_tags_with_conn(&self.conn)
    }

    /// Returns database status metadata stored in `es_meta`.
    pub fn read_system_meta(&self) -> Result<SystemMeta, AppError> {
        self.sync_read_repo().read_system_meta()
    }

    /// Increments database revision and updates write timestamp.
    pub fn bump_revision(&mut self, now: i64) -> Result<SystemMeta, AppError> {
        let tx = self.tx()?;
        let meta = tx.sync_write_repo().bump_revision(now)?;
        tx.commit()?;
        Ok(meta)
    }

    /// Updates the latest sync timestamp and status metadata.
    pub fn update_sync(&mut self, now: i64, status: &str) -> Result<SystemMeta, AppError> {
        let tx = self.tx()?;
        let meta = tx.sync_write_repo().update_sync(now, status)?;
        tx.commit()?;
        Ok(meta)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStore;
    use tempfile::tempdir;

    /// Configures expected SQLite pragmas on open for runtime consistency.
    #[test]
    fn open_sets_expected_pragmas() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("test.sqlite");
        let store = SqliteStore::open(&db_path).expect("open sqlite store");

        let journal_mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal_mode pragma");
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

        let synchronous: i64 = store
            .conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .expect("synchronous pragma");
        assert_eq!(synchronous, 1);
    }

    /// Ensures connection-local foreign key enforcement is enabled on open.
    #[test]
    fn open_enables_foreign_keys() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("test.sqlite");
        let store = SqliteStore::open(&db_path).expect("open sqlite store");
        let foreign_keys: i64 = store
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("foreign_keys pragma");
        assert_eq!(foreign_keys, 1);
    }
}
