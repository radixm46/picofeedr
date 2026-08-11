//! Sync repositories for SQLite-backed metadata operations.

use crate::db::sqlite::meta;
use crate::error::AppError;
use rusqlite::Connection;

/// Read-only repository for sync metadata queries.
pub struct SyncReadRepo<'a> {
    conn: &'a Connection,
}

impl<'a> SyncReadRepo<'a> {
    /// Creates a read repository bound to one SQLite connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Loads database-wide sync metadata.
    pub fn read_system_meta(&self) -> Result<meta::SystemMeta, AppError> {
        meta::read_meta_with_conn(self.conn)
    }
}

/// Write-oriented repository for sync metadata updates.
pub struct SyncWriteRepo<'a> {
    conn: &'a Connection,
}

impl<'a> SyncWriteRepo<'a> {
    /// Creates a write repository bound to one SQLite transaction connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Increments write revision in metadata.
    pub fn bump_revision(&self, now: i64) -> Result<meta::SystemMeta, AppError> {
        meta::bump_revision_with_conn(self.conn, now)
    }

    /// Updates latest sync status in metadata.
    pub fn update_sync(&self, now: i64, status: &str) -> Result<meta::SystemMeta, AppError> {
        meta::update_sync_with_conn(self.conn, now, status)
    }
}
