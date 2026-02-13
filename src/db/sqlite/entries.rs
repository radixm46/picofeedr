//! Entry queries for SQLite store.

use crate::db::sqlite::query::entries as q;
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
        q::INSERT_ENTRY,
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
    let entry_id: i64 = if inserted {
        conn.last_insert_rowid()
    } else {
        conn.query_row(q::SELECT_ENTRY_ID_BY_KEY, params![entry.entry_key], |row| {
            row.get(0)
        })?
    };
    Ok(EntryInsertResult { entry_id, inserted })
}

/// Inserts entry content using a provided connection.
pub(crate) fn insert_entry_content_with_conn(
    conn: &Connection,
    entry_id: i64,
    content: &EntryContentInput,
) -> Result<(), AppError> {
    conn.execute(
        q::UPSERT_ENTRY_CONTENT,
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
    let mut insert_tag_stmt = conn.prepare(q::INSERT_TAG_IGNORE)?;
    for tag in &unique {
        insert_tag_stmt.execute(params![tag])?;
    }
    let placeholders = std::iter::repeat_n("?", unique.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = q::select_tag_ids_by_names(&placeholders);
    let mut stmt = conn.prepare(&query)?;
    let mut rows = stmt.query(params_from_iter(unique.iter()))?;
    let mut tag_ids = HashMap::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        tag_ids.insert(name, id);
    }
    let mut insert_entry_tag_stmt = conn.prepare(q::INSERT_ENTRY_TAG_IGNORE)?;
    for tag in &unique {
        let tag_id = tag_ids
            .get(tag)
            .ok_or_else(|| AppError::db(format!("Missing tag id for {tag}")))?;
        insert_entry_tag_stmt.execute(params![entry_id, tag_id])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{insert_entry_tags_with_conn, insert_entry_with_conn};
    use crate::db::sqlite::feeds::upsert_feed_with_conn;
    use crate::db::sqlite::query::{entries as q_entries, feeds as q_feeds};
    use crate::db::{EntryInput, FeedInput};
    use rusqlite::{Connection, params};

    /// Returns in-memory connection with migrated schema.
    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        crate::db::migrate::migrate(&conn).expect("migrate");
        conn
    }

    /// Inserts a feed and returns its id.
    fn insert_feed(conn: &Connection, feed_key: &str) -> i64 {
        upsert_feed_with_conn(
            conn,
            &FeedInput {
                feed_key: feed_key.to_string(),
                url: format!("https://example.com/{feed_key}"),
                title: Some(feed_key.to_string()),
                author: None,
                site_url: None,
                meta_json: None,
            },
            1,
        )
        .expect("upsert feed");
        conn.query_row(q_feeds::SELECT_FEED_ID_BY_KEY, params![feed_key], |row| {
            row.get(0)
        })
        .expect("feed id")
    }

    /// Keeps entry id stable when inserting duplicate entry_key.
    #[test]
    fn insert_entry_returns_existing_id_on_conflict() {
        let conn = test_conn();
        let feed_id = insert_feed(&conn, "feed-a");
        let input = EntryInput {
            entry_key: "entry-a".to_string(),
            feed_id,
            source_id: Some("src-a".to_string()),
            link: Some("https://example.com/a".to_string()),
            title: Some("A".to_string()),
            author: None,
            published_at: None,
            updated_at: None,
            first_seen_at: 10,
            meta_json: None,
        };
        let first = insert_entry_with_conn(&conn, &input).expect("first insert");
        assert!(first.inserted);
        assert!(first.entry_id > 0);

        let second = insert_entry_with_conn(&conn, &input).expect("second insert");
        assert!(!second.inserted);
        assert_eq!(second.entry_id, first.entry_id);
    }

    /// Deduplicates input tags before writing tags and entry_tags.
    #[test]
    fn insert_entry_tags_deduplicates_tag_inputs() {
        let conn = test_conn();
        let feed_id = insert_feed(&conn, "feed-a");
        let input = EntryInput {
            entry_key: "entry-a".to_string(),
            feed_id,
            source_id: Some("src-a".to_string()),
            link: Some("https://example.com/a".to_string()),
            title: Some("A".to_string()),
            author: None,
            published_at: None,
            updated_at: None,
            first_seen_at: 10,
            meta_json: None,
        };
        let inserted = insert_entry_with_conn(&conn, &input).expect("insert entry");
        let tags = vec![
            "tech".to_string(),
            "tech".to_string(),
            "hot".to_string(),
            "hot".to_string(),
        ];
        insert_entry_tags_with_conn(&conn, inserted.entry_id, &tags).expect("insert tags");

        let tag_count: i64 = conn
            .query_row(q_entries::COUNT_TAGS, [], |row| row.get(0))
            .expect("tag count");
        assert_eq!(tag_count, 2);
        let entry_tag_count: i64 = conn
            .query_row(
                q_entries::COUNT_ENTRY_TAGS_BY_ENTRY_ID,
                params![inserted.entry_id],
                |row| row.get(0),
            )
            .expect("entry_tag count");
        assert_eq!(entry_tag_count, 2);
    }
}
