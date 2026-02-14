//! Sync repositories for SQLite-backed ingest workflows.

use crate::config::AppConfig;
use crate::db::EntryContentStorage;
use crate::db::sqlite::repo::feed_repo::FeedReadRepo;
use crate::db::sqlite::{entries, meta};
use crate::error::AppError;
use crate::sync::content::{remove_content_fs, write_content_fs};
use crate::sync::model::SyncResult;
use rusqlite::Connection;
use std::collections::HashSet;

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

    /// Persists fetched sync results and returns number of newly inserted entries.
    pub(crate) fn ingest_results(
        &self,
        config: &AppConfig,
        results: Vec<SyncResult>,
    ) -> Result<usize, AppError> {
        let feed_keys = collect_unique_feed_keys(&results);
        let feed_pks_by_feed_key =
            FeedReadRepo::new(self.conn).find_feed_pks_by_keys(&feed_keys)?;
        let mut new_entries = 0;
        for result in results {
            for entry in result.entries {
                let feed_pk = feed_pks_by_feed_key
                    .get(&entry.feed_key)
                    .copied()
                    .ok_or_else(|| AppError::db(format!("Missing feed for {}", entry.feed_key)))?;
                let input = entry.entry.with_feed_pk(feed_pk);
                let insert = entries::insert_entry_with_conn(self.conn, &input)?;
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
                            if let Err(error) = entries::insert_entry_content_with_conn(
                                self.conn,
                                insert.entry_pk,
                                content,
                            ) {
                                if created {
                                    let _ = remove_content_fs(&config.storage.data_dir, reference);
                                }
                                return Err(error);
                            }
                        } else {
                            entries::insert_entry_content_with_conn(
                                self.conn,
                                insert.entry_pk,
                                content,
                            )?;
                        }
                    }
                    entries::insert_entry_tags_with_conn(self.conn, insert.entry_pk, &entry.tags)?;
                    new_entries += 1;
                }
            }
        }
        Ok(new_entries)
    }
}

/// Collects unique feed keys from sync results while preserving first-seen order.
fn collect_unique_feed_keys(results: &[SyncResult]) -> Vec<String> {
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for result in results {
        for entry in &result.entries {
            if seen.insert(entry.feed_key.clone()) {
                unique.push(entry.feed_key.clone());
            }
        }
    }
    unique
}
