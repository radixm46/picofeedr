//! Entry queries for SQLite store.

use crate::db::{EntryContentInput, EntryInput, EntryInsertResult};
use crate::error::AppError;
use rusqlite::{Connection, params, params_from_iter};
use std::collections::{HashMap, HashSet};

/// Inserts an entry and returns its ID using a provided connection.
pub(crate) fn insert_entry_with_conn(
    conn: &Connection,
    entry: &EntryInput,
) -> Result<EntryInsertResult, AppError> {
    let inserted = conn.execute(
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
    let entry_id: i64 = conn.query_row(
        "SELECT id FROM entries WHERE entry_key = ?1",
        params![entry.entry_key],
        |row| row.get(0),
    )?;
    Ok(EntryInsertResult { entry_id, inserted })
}

/// Inserts entry content using a provided connection.
pub(crate) fn insert_entry_content_with_conn(
    conn: &Connection,
    entry_id: i64,
    content: &EntryContentInput,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO entry_contents (entry_id, storage, ref, content_type, content)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            entry_id,
            content.storage.as_str(),
            content.reference,
            content.content_type,
            content.content
        ],
    )?;
    Ok(())
}

/// Inserts tags for an entry using a provided connection.
pub(crate) fn insert_entry_tags_with_conn(
    conn: &Connection,
    entry_id: i64,
    tags: &[String],
) -> Result<(), AppError> {
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
        conn.execute(
            "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
            params![tag],
        )?;
    }
    let placeholders = std::iter::repeat_n("?", unique.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!("SELECT id, name FROM tags WHERE name IN ({placeholders})");
    let mut stmt = conn.prepare(&query)?;
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
        conn.execute(
            "INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?1, ?2)",
            params![entry_id, tag_id],
        )?;
    }
    Ok(())
}
