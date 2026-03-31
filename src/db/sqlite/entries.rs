//! Entry DAO for SQLite store.
//!
//! This module intentionally stays at single-statement query execution level.
//! Multi-step workflows must live in repository modules.

use crate::db::sqlite::query::entries as q;
use crate::db::{EntryContentInput, EntryInput, EntryInsertResult};
use crate::error::AppError;
use crate::string_set::dedupe_strings_preserve_order;
use rusqlite::{Connection, Statement, params, params_from_iter};
use std::collections::HashMap;

const TAG_ID_LOOKUP_CHUNK_SIZE: usize = 64;

/// Prepared ingest statements reused across many entry writes.
pub(crate) struct IngestContext<'conn> {
    conn: &'conn Connection,
    insert_entry_stmt: Statement<'conn>,
    select_entry_pk_stmt: Statement<'conn>,
    upsert_entry_content_stmt: Statement<'conn>,
    insert_tag_stmt: Statement<'conn>,
    insert_entry_tag_stmt: Statement<'conn>,
}

impl<'conn> IngestContext<'conn> {
    /// Creates a prepared statement context for ingest operations.
    pub(crate) fn new(conn: &'conn Connection) -> Result<Self, AppError> {
        Ok(Self {
            conn,
            insert_entry_stmt: conn.prepare(q::INSERT_ENTRY)?,
            select_entry_pk_stmt: conn.prepare(q::SELECT_ENTRY_PK_BY_ID)?,
            upsert_entry_content_stmt: conn.prepare(q::UPSERT_ENTRY_CONTENT)?,
            insert_tag_stmt: conn.prepare(q::INSERT_TAG_IGNORE)?,
            insert_entry_tag_stmt: conn.prepare(q::INSERT_ENTRY_TAG_IGNORE)?,
        })
    }

    /// Inserts an entry and returns its id.
    pub(crate) fn insert_entry(
        &mut self,
        entry: &EntryInput,
    ) -> Result<EntryInsertResult, AppError> {
        let inserted = self.insert_entry_stmt.execute(params![
            entry.entry_id,
            entry.feed_pk,
            entry.link,
            entry.title,
            entry.author,
            entry.published_at,
            entry.updated_at,
            entry.first_seen_at,
            entry.meta_json
        ])? > 0;
        let entry_pk: i64 = if inserted {
            self.conn.last_insert_rowid()
        } else {
            self.select_entry_pk_stmt
                .query_row(params![entry.entry_id], |row| row.get(0))?
        };
        Ok(EntryInsertResult { entry_pk, inserted })
    }

    /// Inserts or updates entry content for an entry.
    pub(crate) fn insert_entry_content(
        &mut self,
        entry_pk: i64,
        content: &EntryContentInput,
    ) -> Result<(), AppError> {
        self.upsert_entry_content_stmt.execute(params![
            entry_pk,
            content.storage.as_str(),
            content.reference,
            content.content_type,
            content.content
        ])?;
        Ok(())
    }

    /// Inserts deduplicated tags and entry-tag relations for an entry.
    pub(crate) fn insert_entry_tags(
        &mut self,
        entry_pk: i64,
        tags: &[String],
    ) -> Result<(), AppError> {
        if tags.is_empty() {
            return Ok(());
        }
        let unique = dedupe_strings_preserve_order(tags.iter().cloned());
        for tag in &unique {
            self.insert_tag_stmt.execute(params![tag])?;
        }
        let tag_ids = resolve_tag_ids(self.conn, &unique)?;
        for tag in &unique {
            let tag_id = tag_ids
                .get(tag)
                .ok_or_else(|| AppError::db(format!("Missing tag id for {tag}")))?;
            self.insert_entry_tag_stmt
                .execute(params![entry_pk, tag_id])?;
        }
        Ok(())
    }
}

fn resolve_tag_ids(conn: &Connection, unique: &[String]) -> Result<HashMap<String, i64>, AppError> {
    let mut tag_ids = HashMap::new();
    for chunk in unique.chunks(TAG_ID_LOOKUP_CHUNK_SIZE) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let query = q::select_tag_ids_by_names(&placeholders);
        let mut stmt = conn.prepare_cached(&query)?;
        let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            tag_ids.insert(name, id);
        }
    }
    Ok(tag_ids)
}

#[cfg(test)]
mod tests {
    use super::IngestContext;
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
    fn insert_feed(conn: &Connection, feed_id: &str) -> i64 {
        upsert_feed_with_conn(
            conn,
            &FeedInput {
                feed_id: feed_id.to_string(),
                url: format!("https://example.com/{feed_id}"),
                title: Some(feed_id.to_string()),
                author: None,
                site_url: None,
                meta_json: None,
            },
            1,
        )
        .expect("upsert feed");
        conn.query_row(q_feeds::SELECT_FEED_PK_BY_ID, params![feed_id], |row| {
            row.get(0)
        })
        .expect("feed pk")
    }

    /// Keeps entry id stable when inserting duplicate entry_id.
    #[test]
    fn insert_entry_returns_existing_id_on_conflict() {
        let conn = test_conn();
        let feed_pk = insert_feed(&conn, "feed-a");
        let mut ingest = IngestContext::new(&conn).expect("create ingest context");
        let input = EntryInput {
            entry_id: "entry-a".to_string(),
            feed_pk,
            link: Some("https://example.com/a".to_string()),
            title: Some("A".to_string()),
            author: None,
            published_at: None,
            updated_at: None,
            first_seen_at: 10,
            meta_json: None,
        };
        let first = ingest.insert_entry(&input).expect("first insert");
        assert!(first.inserted);
        assert!(first.entry_pk > 0);

        let second = ingest.insert_entry(&input).expect("second insert");
        assert!(!second.inserted);
        assert_eq!(second.entry_pk, first.entry_pk);
    }

    /// Deduplicates input tags before writing tags and entry_tags.
    #[test]
    fn insert_entry_tags_deduplicates_tag_inputs() {
        let conn = test_conn();
        let feed_pk = insert_feed(&conn, "feed-a");
        let mut ingest = IngestContext::new(&conn).expect("create ingest context");
        let input = EntryInput {
            entry_id: "entry-a".to_string(),
            feed_pk,
            link: Some("https://example.com/a".to_string()),
            title: Some("A".to_string()),
            author: None,
            published_at: None,
            updated_at: None,
            first_seen_at: 10,
            meta_json: None,
        };
        let inserted = ingest.insert_entry(&input).expect("insert entry");
        let tags = vec![
            "tech".to_string(),
            "tech".to_string(),
            "hot".to_string(),
            "hot".to_string(),
        ];
        ingest
            .insert_entry_tags(inserted.entry_pk, &tags)
            .expect("insert tags");

        let tag_count: i64 = conn
            .query_row(q_entries::COUNT_TAGS, [], |row| row.get(0))
            .expect("tag count");
        assert_eq!(tag_count, 2);
        let entry_tag_count: i64 = conn
            .query_row(
                q_entries::COUNT_ENTRY_TAGS_BY_ENTRY_ID,
                params![inserted.entry_pk],
                |row| row.get(0),
            )
            .expect("entry_tag count");
        assert_eq!(entry_tag_count, 2);
    }

    /// Keeps tag cardinality stable across multiple sequential entry inserts.
    #[test]
    fn insert_entry_tags_multiple_entries_keep_distinct_tags() {
        let conn = test_conn();
        let feed_pk = insert_feed(&conn, "feed-a");
        let mut ingest = IngestContext::new(&conn).expect("create ingest context");
        let first = ingest
            .insert_entry(&EntryInput {
                entry_id: "entry-a".to_string(),
                feed_pk,
                link: Some("https://example.com/a".to_string()),
                title: Some("A".to_string()),
                author: None,
                published_at: None,
                updated_at: None,
                first_seen_at: 10,
                meta_json: None,
            })
            .expect("insert first entry");
        ingest
            .insert_entry_tags(
                first.entry_pk,
                &["tech".to_string(), "tech".to_string(), "rust".to_string()],
            )
            .expect("insert first tags");

        let second = ingest
            .insert_entry(&EntryInput {
                entry_id: "entry-b".to_string(),
                feed_pk,
                link: Some("https://example.com/b".to_string()),
                title: Some("B".to_string()),
                author: None,
                published_at: None,
                updated_at: None,
                first_seen_at: 11,
                meta_json: None,
            })
            .expect("insert second entry");
        ingest
            .insert_entry_tags(
                second.entry_pk,
                &["rust".to_string(), "ops".to_string(), "ops".to_string()],
            )
            .expect("insert second tags");

        let tag_count: i64 = conn
            .query_row(q_entries::COUNT_TAGS, [], |row| row.get(0))
            .expect("tag count");
        assert_eq!(tag_count, 3);

        let first_entry_tag_count: i64 = conn
            .query_row(
                q_entries::COUNT_ENTRY_TAGS_BY_ENTRY_ID,
                params![first.entry_pk],
                |row| row.get(0),
            )
            .expect("first entry_tag count");
        assert_eq!(first_entry_tag_count, 2);

        let second_entry_tag_count: i64 = conn
            .query_row(
                q_entries::COUNT_ENTRY_TAGS_BY_ENTRY_ID,
                params![second.entry_pk],
                |row| row.get(0),
            )
            .expect("second entry_tag count");
        assert_eq!(second_entry_tag_count, 2);
    }

    /// Resolves tag ids correctly when tag count crosses lookup chunk boundaries.
    #[test]
    fn insert_entry_tags_resolves_ids_across_chunks() {
        let conn = test_conn();
        let feed_pk = insert_feed(&conn, "feed-a");
        let mut ingest = IngestContext::new(&conn).expect("create ingest context");
        let inserted = ingest
            .insert_entry(&EntryInput {
                entry_id: "entry-chunk".to_string(),
                feed_pk,
                link: Some("https://example.com/chunk".to_string()),
                title: Some("Chunk".to_string()),
                author: None,
                published_at: None,
                updated_at: None,
                first_seen_at: 12,
                meta_json: None,
            })
            .expect("insert entry");
        let tags = (0..80)
            .map(|index| format!("tag-{index}"))
            .collect::<Vec<_>>();

        ingest
            .insert_entry_tags(inserted.entry_pk, &tags)
            .expect("insert many tags");

        let tag_count: i64 = conn
            .query_row(q_entries::COUNT_TAGS, [], |row| row.get(0))
            .expect("tag count");
        assert_eq!(tag_count, 80);
        let entry_tag_count: i64 = conn
            .query_row(
                q_entries::COUNT_ENTRY_TAGS_BY_ENTRY_ID,
                params![inserted.entry_pk],
                |row| row.get(0),
            )
            .expect("entry tag count");
        assert_eq!(entry_tag_count, 80);
    }
}
