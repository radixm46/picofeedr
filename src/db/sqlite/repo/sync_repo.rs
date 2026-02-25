//! Sync repositories for SQLite-backed ingest workflows.

use crate::config::AppConfig;
use crate::db::EntryContentStorage;
use crate::db::sqlite::{entries, meta};
use crate::error::AppError;
use crate::sync::content::{remove_content_fs, write_content_fs};
use crate::sync::model::SyncResult;
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

/// Write-oriented repository for sync metadata updates and ingest workflows.
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

    /// Persists one feed result and returns number of newly inserted entries.
    pub(crate) fn ingest_feed_result(
        &self,
        config: &AppConfig,
        feed_pk: i64,
        result: SyncResult,
    ) -> Result<usize, AppError> {
        let mut ingest = entries::IngestContext::new(self.conn)?;
        let mut new_entries = 0;
        for entry in result.entries {
            let input = entry.entry.with_feed_pk(feed_pk);
            let insert = ingest.insert_entry(&input)?;
            if insert.inserted {
                if let Some(content) = entry.content.as_ref() {
                    if content.storage == EntryContentStorage::Fs {
                        let payload = entry.content_payload.as_deref().ok_or_else(|| {
                            AppError::internal("Missing content payload for fs storage")
                        })?;
                        let reference = content.reference.as_deref().ok_or_else(|| {
                            AppError::internal("Missing content reference for fs storage")
                        })?;
                        let created =
                            write_content_fs(&config.storage.data_dir, reference, payload)?;
                        if let Err(error) = ingest.insert_entry_content(insert.entry_pk, content) {
                            if created {
                                let _ = remove_content_fs(&config.storage.data_dir, reference);
                            }
                            return Err(error);
                        }
                    } else {
                        ingest.insert_entry_content(insert.entry_pk, content)?;
                    }
                }
                ingest.insert_entry_tags(insert.entry_pk, &entry.tags)?;
                new_entries += 1;
            }
        }
        Ok(new_entries)
    }
}
