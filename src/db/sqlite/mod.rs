//! SQLite adapter for the store.

mod entries;
mod feeds;
mod meta;
pub(crate) mod query;
pub mod repo;
pub(crate) mod schema;
mod tags;

use crate::db::sqlite::repo::{EntryReadRepo, EntryWriteRepo};
use crate::db::{FeedInput, FeedMetadataInput, FeedRow};
use crate::error::{AppError, error_details};
use rusqlite::{Connection, TransactionBehavior};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

pub(crate) use meta::{SystemMeta, initialize_meta_with_conn};

/// SQLite transaction wrapper.
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

    /// Ensures configured feeds exist in SQLite.
    pub(crate) fn ensure_feeds(&self, feed_inputs: &[FeedInput], now: i64) -> Result<(), AppError> {
        for feed in feed_inputs {
            feeds::upsert_feed_from_config_with_conn(&self.tx, feed, now)?;
        }
        Ok(())
    }

    /// Refreshes observed feed metadata on an existing feed row.
    pub(crate) fn refresh_feed_metadata(
        &self,
        feed_pk: i64,
        metadata: &FeedMetadataInput,
        now: i64,
    ) -> Result<(), AppError> {
        feeds::refresh_feed_metadata_with_conn(
            &self.tx,
            feed_pk,
            metadata.title.as_deref(),
            metadata.author.as_deref(),
            metadata.site_url.as_deref(),
            now,
        )
    }

    /// Creates a prepared entry-ingest context scoped to this transaction.
    pub(crate) fn ingest_context(&self) -> Result<entries::IngestContext<'_>, AppError> {
        entries::IngestContext::new(&self.tx)
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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                AppError::io_with_details_and_source(
                    format!("Failed to create storage root directory: {error}"),
                    error_details([
                        ("path", Value::from(parent.to_string_lossy().to_string())),
                        ("db_path", Value::from(path.to_string_lossy().to_string())),
                        ("hint", Value::from("failed_to_create_storage_root")),
                    ]),
                    error,
                )
            })?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        // NOTE: Consider making busy_timeout configurable once needed.
        Ok(Self { conn })
    }

    /// Begins a wrapped transaction for write operations.
    pub fn tx(&mut self) -> Result<Tx<'_>, AppError> {
        Ok(Tx::new(self.conn.transaction()?))
    }

    /// Begins an immediate transaction for recovery checks that must serialize writers.
    pub(crate) fn immediate_tx(&mut self) -> Result<Tx<'_>, AppError> {
        Ok(Tx::new(self.conn.transaction_with_behavior(
            TransactionBehavior::Immediate,
        )?))
    }

    /// Returns a read-only entry repository bound to the store connection.
    pub fn entry_read_repo(&self) -> EntryReadRepo<'_> {
        EntryReadRepo::new(&self.conn)
    }

    /// Applies schema migrations.
    pub fn migrate(&self) -> Result<(), AppError> {
        crate::db::migrate::migrate(&self.conn)
    }

    /// Returns the on-disk SQLite schema version.
    pub fn schema_version(&self) -> Result<i64, AppError> {
        crate::db::migrate::read_schema_version(&self.conn)
    }

    /// Returns all feeds stored in the database.
    pub fn list_feeds(&self) -> Result<Vec<FeedRow>, AppError> {
        feeds::list_feeds_with_conn(&self.conn)
    }

    /// Resolves feed primary keys by feed ids.
    pub(crate) fn find_feed_pks_by_ids(
        &self,
        feed_ids: &[String],
    ) -> Result<HashMap<String, i64>, AppError> {
        feeds::find_feed_pks_by_ids_with_conn(&self.conn, feed_ids)
    }

    /// Lists all tags ordered by name.
    pub fn list_tags(&self) -> Result<Vec<String>, AppError> {
        tags::list_tags_with_conn(&self.conn)
    }

    /// Returns database status metadata stored in `es_meta`.
    pub fn read_system_meta(&self) -> Result<SystemMeta, AppError> {
        meta::read_meta_with_conn(&self.conn)
    }

    /// Increments database revision and updates write timestamp.
    pub fn bump_revision(&mut self, now: i64) -> Result<SystemMeta, AppError> {
        let tx = self.tx()?;
        let meta = meta::bump_revision_with_conn(&tx.tx, now)?;
        tx.commit()?;
        Ok(meta)
    }

    /// Updates the latest sync timestamp and status metadata.
    pub fn update_sync(&mut self, now: i64, status: &str) -> Result<SystemMeta, AppError> {
        let tx = self.tx()?;
        let meta = meta::update_sync_with_conn(&tx.tx, now, status)?;
        tx.commit()?;
        Ok(meta)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStore;
    use serde_json::Value;
    use std::fs;
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

    #[test]
    fn open_returns_contextual_error_when_storage_root_creation_fails() {
        let dir = tempdir().expect("tempdir");
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, "not a directory").expect("write blocker file");
        let db_path = blocked.join("db.sqlite");

        let error = match SqliteStore::open(&db_path) {
            Ok(_) => panic!("open should fail"),
            Err(error) => error,
        };

        assert_eq!(error.code().as_str(), "IO_ERROR");
        let details = error.details().expect("details");
        assert_eq!(details["hint"], "failed_to_create_storage_root");
        assert!(
            details
                .get("path")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        );
        assert!(
            details
                .get("db_path")
                .and_then(Value::as_str)
                .is_some_and(|value| value.ends_with("db.sqlite"))
        );
    }
}
