//! Entry repositories for SQLite-backed listing and mutation operations.

use crate::db::EntryContentStorage as Storage;
use crate::db::sqlite::query::entries as q;
use crate::db::sqlite::tags;
use crate::entry::{EntryEnclosure, EntrySummary};
use crate::error::AppError;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// Tuple payload for the entry detail base row selected from SQLite.
pub(crate) type EntryDetailRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    i64,
);

/// One list row with metadata required to finalize response payloads.
pub(crate) struct EntryListRow {
    /// Internal entry id used for joins and tag loading.
    pub entry_pk: i64,
    /// Public summary payload for JSON/plain rendering.
    pub summary: EntrySummary,
    /// Feed title resolved from feeds table.
    pub feed_title: Option<String>,
}

/// Read-only repository for entry query operations.
pub struct EntryReadRepo<'a> {
    conn: &'a Connection,
}

impl<'a> EntryReadRepo<'a> {
    const IN_CHUNK_SIZE: usize = 500;

    /// Creates a read repository bound to one SQLite connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Resolves internal entry primary keys keyed by stable entry id.
    pub fn find_entry_pks_by_ids(
        &self,
        entry_ids: &[String],
    ) -> Result<HashMap<String, i64>, AppError> {
        let mut ids = HashMap::new();
        for chunk in entry_ids.chunks(Self::IN_CHUNK_SIZE) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = q::select_entry_pks_by_ids(&placeholders);
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
            while let Some(row) = rows.next()? {
                let id: i64 = row.get(0)?;
                let entry_id: String = row.get(1)?;
                ids.insert(entry_id, id);
            }
        }
        Ok(ids)
    }

    /// Lists `(entry_pk, sort_key)` tuples using non-tag where filters.
    pub fn list_filtered_entry_sort_keys(
        &self,
        where_sql: &str,
        params: &[Value],
        key_expr: &str,
    ) -> Result<Vec<(i64, i64)>, AppError> {
        let sql = q::select_filtered_entry_sort_keys(where_sql, key_expr);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(params.iter()))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let entry_pk: i64 = row.get(0)?;
            let sort_key: i64 = row.get(1)?;
            out.push((entry_pk, sort_key));
        }
        Ok(out)
    }

    /// Loads list rows for explicitly specified entry primary keys.
    pub(crate) fn load_entry_rows_by_entry_pks(
        &self,
        entry_pks: &[i64],
    ) -> Result<Vec<EntryListRow>, AppError> {
        if entry_pks.is_empty() {
            return Ok(Vec::new());
        }
        let mut rows_out = Vec::with_capacity(entry_pks.len());
        for chunk in entry_pks.chunks(Self::IN_CHUNK_SIZE) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = q::select_entry_rows_by_entry_pks(&placeholders);
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
            while let Some(row) = rows.next()? {
                let entry_pk: i64 = row.get(0)?;
                let entry_id: String = row.get(1)?;
                let feed_id: String = row.get(2)?;
                let feed_title: Option<String> = row.get(3)?;
                let title: Option<String> = row.get(4)?;
                let link: Option<String> = row.get(5)?;
                let published_at: Option<i64> = row.get(6)?;
                let first_seen_at: i64 = row.get(7)?;
                rows_out.push(EntryListRow {
                    entry_pk,
                    summary: EntrySummary {
                        entry_id,
                        feed_id,
                        title,
                        link,
                        published_at,
                        first_seen_at,
                        tags: Vec::new(),
                    },
                    feed_title,
                });
            }
        }
        Ok(rows_out)
    }

    /// Loads entry primary keys by tag ids in fixed-size chunks.
    pub fn find_entry_pks_by_tag_ids(
        &self,
        tag_ids: &[i64],
    ) -> Result<HashMap<i64, HashSet<i64>>, AppError> {
        if tag_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut map: HashMap<i64, HashSet<i64>> = HashMap::new();
        for tag_chunk in tag_ids.chunks(Self::IN_CHUNK_SIZE) {
            let tag_placeholders = std::iter::repeat_n("?", tag_chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = q::select_entry_pks_by_tag_ids(&tag_placeholders);
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(tag_chunk.iter()))?;
            while let Some(row) = rows.next()? {
                let tag_id: i64 = row.get(0)?;
                let entry_pk: i64 = row.get(1)?;
                map.entry(tag_id).or_default().insert(entry_pk);
            }
        }
        Ok(map)
    }

    /// Ensures all requested entry ids exist.
    pub fn ensure_all_entry_ids_exist(&self, entry_ids: &[String]) -> Result<(), AppError> {
        let existing = self.find_entry_pks_by_ids(entry_ids)?;
        for entry_id in entry_ids {
            if !existing.contains_key(entry_id) {
                return Err(AppError::entry_not_found("some entries not found"));
            }
        }
        Ok(())
    }

    /// Counts entries with an optional where clause.
    pub fn count_entries(&self, where_sql: &str, params: &[Value]) -> Result<i64, AppError> {
        let sql = q::count_entries(where_sql);
        let total: i64 = self
            .conn
            .query_row(&sql, params_from_iter(params.iter()), |row| row.get(0))?;
        Ok(total)
    }

    /// Fetches one list page with an already-built where/order contract.
    pub(crate) fn fetch_entries(
        &self,
        where_sql: &str,
        params: &[Value],
        key_expr: &str,
        order_clause: &str,
        limit: usize,
    ) -> Result<(Vec<EntryListRow>, Vec<i64>), AppError> {
        let fetch_limit = limit.saturating_add(1);
        let sql = q::fetch_entries(where_sql, key_expr, order_clause);
        let mut list_params = params.to_vec();
        list_params.push(Value::from(fetch_limit as i64));
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(list_params.iter()))?;
        let mut entries = Vec::new();
        let mut sort_keys = Vec::new();
        while let Some(row) = rows.next()? {
            let entry_pk: i64 = row.get(0)?;
            let entry_id: String = row.get(1)?;
            let feed_id: String = row.get(2)?;
            let feed_title: Option<String> = row.get(3)?;
            let title: Option<String> = row.get(4)?;
            let link: Option<String> = row.get(5)?;
            let published_at: Option<i64> = row.get(6)?;
            let first_seen_at: i64 = row.get(7)?;
            let sort_key: i64 = row.get(8)?;
            entries.push(EntryListRow {
                entry_pk,
                summary: EntrySummary {
                    entry_id,
                    feed_id,
                    title,
                    link,
                    published_at,
                    first_seen_at,
                    tags: Vec::new(),
                },
                feed_title,
            });
            sort_keys.push(sort_key);
        }
        Ok((entries, sort_keys))
    }

    /// Loads tags grouped by entry primary key.
    pub fn load_tags(&self, entry_pks: &[i64]) -> Result<HashMap<i64, Vec<String>>, AppError> {
        if entry_pks.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = std::iter::repeat_n("?", entry_pks.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = q::load_tags_by_entry_ids(&placeholders);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(entry_pks.iter()))?;
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        while let Some(row) = rows.next()? {
            let entry_pk: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            tags.entry(entry_pk).or_default().push(name);
        }
        Ok(tags)
    }

    /// Resolves tag ids keyed by name for the given name slice.
    pub fn find_tag_ids_by_names(
        &self,
        names: &[String],
    ) -> Result<HashMap<String, i64>, AppError> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = std::iter::repeat_n("?", names.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = q::select_tag_ids_by_names(&placeholders);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(names.iter()))?;
        let mut map = HashMap::new();
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            map.insert(name, id);
        }
        Ok(map)
    }

    /// Loads one entry detail row tuple for view operation.
    pub fn view_entry_row(&self, entry_id: &str) -> Result<Option<EntryDetailRow>, AppError> {
        self.conn
            .query_row(q::SELECT_ENTRY_DETAIL_BY_ID, params![entry_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })
            .optional()
            .map_err(AppError::from)
    }

    /// Loads content payload and content type for one entry.
    pub fn load_content(
        &self,
        data_dir: &Path,
        entry_pk: i64,
    ) -> Result<(Option<String>, Option<String>), AppError> {
        let row = self
            .conn
            .query_row(
                q::SELECT_ENTRY_CONTENT_BY_ENTRY_ID,
                params![entry_pk],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((storage, reference, content_type, content)) = row else {
            return Ok((None, None));
        };
        let storage = storage
            .parse::<Storage>()
            .map_err(|_| AppError::internal(format!("Unknown content storage: {storage}")))?;
        match storage {
            Storage::Db => Ok((content, content_type)),
            Storage::Fs => {
                let Some(reference) = reference else {
                    return Ok((None, content_type));
                };
                let path = match crate::content_ref::sha256_path(data_dir, &reference) {
                    Ok(path) => path,
                    Err(_) => return Ok((None, content_type)),
                };
                match fs::read_to_string(path) {
                    Ok(content) => Ok((Some(content), content_type)),
                    Err(error) if error.kind() == ErrorKind::NotFound => Ok((None, content_type)),
                    Err(error) => Err(AppError::io_with_source(
                        "Failed to read entry content",
                        error,
                    )),
                }
            }
            Storage::None => Ok((None, content_type)),
        }
    }

    /// Loads enclosures for one entry.
    pub fn load_enclosures(&self, entry_pk: i64) -> Result<Vec<EntryEnclosure>, AppError> {
        let mut stmt = self.conn.prepare(q::SELECT_ENTRY_ENCLOSURES_BY_ENTRY_ID)?;
        let mut rows = stmt.query(params![entry_pk])?;
        let mut enclosures = Vec::new();
        while let Some(row) = rows.next()? {
            enclosures.push(EntryEnclosure {
                url: row.get(0)?,
                mime_type: row.get(1)?,
                length: row.get(2)?,
            });
        }
        Ok(enclosures)
    }
}

/// Write-oriented repository for entry mutations.
pub struct EntryWriteRepo<'a> {
    conn: &'a Connection,
}

impl<'a> EntryWriteRepo<'a> {
    /// Creates a write repository bound to one SQLite transaction connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Resolves internal entry primary keys keyed by stable entry id.
    pub fn find_entry_pks_by_ids(
        &self,
        entry_ids: &[String],
    ) -> Result<HashMap<String, i64>, AppError> {
        EntryReadRepo::new(self.conn).find_entry_pks_by_ids(entry_ids)
    }

    /// Ensures all requested entry ids exist.
    pub fn ensure_all_entry_ids_exist(&self, entry_ids: &[String]) -> Result<(), AppError> {
        EntryReadRepo::new(self.conn).ensure_all_entry_ids_exist(entry_ids)
    }

    /// Ensures and resolves tag ids for add operation.
    pub fn ensure_tag_ids(&self, tags_in: &[String]) -> Result<HashMap<String, i64>, AppError> {
        tags::ensure_tag_ids_with_conn(self.conn, tags_in)
    }

    /// Resolves existing tag ids for remove operation.
    pub fn lookup_tag_ids(&self, tags_in: &[String]) -> Result<HashMap<String, i64>, AppError> {
        tags::lookup_tag_ids_with_conn(self.conn, tags_in)
    }

    /// Inserts one entry-tag relation if missing.
    pub fn insert_entry_tag(&self, entry_pk: i64, tag_id: i64) -> Result<usize, AppError> {
        let rows = self
            .conn
            .execute(q::INSERT_ENTRY_TAG_IGNORE, params![entry_pk, tag_id])?;
        Ok(rows)
    }

    /// Deletes one entry-tag relation.
    pub fn delete_entry_tag(&self, entry_pk: i64, tag_id: i64) -> Result<usize, AppError> {
        let rows = self
            .conn
            .execute(q::DELETE_ENTRY_TAG, params![entry_pk, tag_id])?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::EntryReadRepo;
    use rusqlite::Connection;
    use std::fs;
    use tempfile::TempDir;

    fn create_entry_contents_table(conn: &Connection) {
        conn.execute(
            "CREATE TABLE entry_contents (
                entry_pk INTEGER PRIMARY KEY,
                storage TEXT NOT NULL,
                ref TEXT,
                content_type TEXT,
                content TEXT
            )",
            [],
        )
        .expect("create entry_contents table");
    }

    #[test]
    fn view_entry_row_returns_entry_pk_with_detail_fields() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        conn.execute(
            "CREATE TABLE feeds (id INTEGER PRIMARY KEY, feed_id TEXT NOT NULL, title TEXT)",
            [],
        )
        .expect("create feeds table");
        conn.execute(
            "CREATE TABLE entries (
                id INTEGER PRIMARY KEY,
                entry_id TEXT NOT NULL,
                feed_pk INTEGER NOT NULL,
                title TEXT,
                link TEXT,
                author TEXT,
                published_at INTEGER,
                first_seen_at INTEGER NOT NULL
            )",
            [],
        )
        .expect("create entries table");
        conn.execute(
            "INSERT INTO feeds (id, feed_id, title) VALUES (1, 'feed-1', 'Feed Title')",
            [],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO entries (id, entry_id, feed_pk, title, link, author, published_at, first_seen_at)
             VALUES (42, 'entry-1', 1, 'Entry Title', 'https://example.com/e1', 'Alice', 1704067200, 1704067200)",
            [],
        )
        .expect("insert entry");

        let row = EntryReadRepo::new(&conn)
            .view_entry_row("entry-1")
            .expect("view row")
            .expect("row exists");

        assert_eq!(row.0, 42);
        assert_eq!(row.1, "entry-1");
        assert_eq!(row.2, "feed-1");
        assert_eq!(row.3.as_deref(), Some("Feed Title"));
        assert_eq!(row.4.as_deref(), Some("Entry Title"));
    }

    #[test]
    fn load_content_fs_not_found_returns_none_with_content_type() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_entry_contents_table(&conn);
        let reference = "a".repeat(64);
        conn.execute(
            "INSERT INTO entry_contents (entry_pk, storage, ref, content_type, content)
             VALUES (1, 'fs', ?1, 'text/html', NULL)",
            [&reference],
        )
        .expect("insert entry content");
        let temp = TempDir::new().expect("tempdir");

        let (content, content_type) = EntryReadRepo::new(&conn)
            .load_content(temp.path(), 1)
            .expect("load content");

        assert_eq!(content, None);
        assert_eq!(content_type.as_deref(), Some("text/html"));
    }

    #[test]
    fn load_content_fs_read_error_returns_io_error() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_entry_contents_table(&conn);
        let reference = "a".repeat(64);
        conn.execute(
            "INSERT INTO entry_contents (entry_pk, storage, ref, content_type, content)
             VALUES (1, 'fs', ?1, 'text/html', NULL)",
            [&reference],
        )
        .expect("insert entry content");
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("aa").join(&reference);
        fs::create_dir_all(&path).expect("create directory at content path");

        let error = EntryReadRepo::new(&conn)
            .load_content(temp.path(), 1)
            .expect_err("directory read should fail");

        assert_eq!(error.code().as_str(), "IO_ERROR");
        assert!(error.to_string().contains("Failed to read entry content"));
    }
}
