//! Database ingestion for sync results.

use crate::db::sqlite::{
    find_feed_id_with_conn, insert_entry_content_with_conn, insert_entry_tags_with_conn,
    insert_entry_with_conn,
};
use crate::error::AppError;
use rusqlite::Connection;

use super::model::SyncResult;

/// Applies normalized entries to the database and returns the number of new inserts.
pub(crate) fn ingest_results(
    conn: &Connection,
    results: Vec<SyncResult>,
) -> Result<usize, AppError> {
    let mut new_entries = 0;
    for result in results {
        for entry in result.entries {
            let feed_id = find_feed_id_with_conn(conn, &entry.feed_key)?
                .ok_or_else(|| AppError::db(format!("Missing feed for {}", entry.feed_key)))?;
            let input = entry.entry.with_feed_id(feed_id);
            let insert = insert_entry_with_conn(conn, &input)?;
            if insert.inserted {
                if let Some(content) = entry.content {
                    insert_entry_content_with_conn(conn, insert.entry_id, &content)?;
                }
                insert_entry_tags_with_conn(conn, insert.entry_id, &entry.tags)?;
                new_entries += 1;
            }
        }
    }
    Ok(new_entries)
}
