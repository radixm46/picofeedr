//! Database ingestion for sync results.

use crate::config::AppConfig;
use crate::db::EntryContentStorage;
use crate::db::sqlite::{
    find_feed_ids_with_conn, insert_entry_content_with_conn, insert_entry_tags_with_conn,
    insert_entry_with_conn,
};
use crate::error::AppError;
use rusqlite::Connection;
use std::collections::HashSet;

use super::content::{remove_content_fs, write_content_fs};
use super::model::SyncResult;

/// Applies normalized entries to the database and returns the number of new inserts.
pub(crate) fn ingest_results(
    conn: &Connection,
    config: &AppConfig,
    results: Vec<SyncResult>,
) -> Result<usize, AppError> {
    let feed_keys = collect_unique_feed_keys(&results);
    let feed_ids = find_feed_ids_with_conn(conn, &feed_keys)?;
    let mut new_entries = 0;
    for result in results {
        for entry in result.entries {
            let feed_id = feed_ids
                .get(&entry.feed_key)
                .copied()
                .ok_or_else(|| AppError::db(format!("Missing feed for {}", entry.feed_key)))?;
            let input = entry.entry.with_feed_id(feed_id);
            let insert = insert_entry_with_conn(conn, &input)?;
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
                        if let Err(error) =
                            insert_entry_content_with_conn(conn, insert.entry_id, content)
                        {
                            if created {
                                let _ = remove_content_fs(&config.storage.data_dir, reference);
                            }
                            return Err(error);
                        }
                    } else {
                        insert_entry_content_with_conn(conn, insert.entry_id, content)?;
                    }
                }
                insert_entry_tags_with_conn(conn, insert.entry_id, &entry.tags)?;
                new_entries += 1;
            }
        }
    }
    Ok(new_entries)
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
