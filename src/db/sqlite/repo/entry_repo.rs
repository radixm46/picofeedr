//! Entry repositories for SQLite-backed listing and mutation operations.

use crate::db::EntryContentStorage as Storage;
use crate::db::sqlite::query::entries as q;
use crate::db::sqlite::tags;
use crate::entry::{EntryEnclosure, EntrySummary};
use crate::error::AppError;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Tuple payload for the entry detail base row selected from SQLite.
pub(crate) type EntryDetailRow = (
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
    pub internal_id: i64,
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
    /// Creates a read repository bound to one SQLite connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Resolves internal entry ids keyed by stable entry key.
    pub fn find_entry_ids_by_keys(
        &self,
        entry_keys: &[String],
    ) -> Result<HashMap<String, i64>, AppError> {
        const ENTRY_KEY_CHUNK_SIZE: usize = 500;

        let mut ids = HashMap::new();
        for chunk in entry_keys.chunks(ENTRY_KEY_CHUNK_SIZE) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = q::select_entry_ids_by_keys(&placeholders);
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
            while let Some(row) = rows.next()? {
                let id: i64 = row.get(0)?;
                let key: String = row.get(1)?;
                ids.insert(key, id);
            }
        }
        Ok(ids)
    }

    /// Ensures all requested entry keys exist.
    pub fn ensure_all_entry_keys_exist(&self, entry_keys: &[String]) -> Result<(), AppError> {
        let existing = self.find_entry_ids_by_keys(entry_keys)?;
        for entry_key in entry_keys {
            if !existing.contains_key(entry_key) {
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
            let id: i64 = row.get(0)?;
            let entry_id: String = row.get(1)?;
            let feed_id: String = row.get(2)?;
            let feed_title: Option<String> = row.get(3)?;
            let title: Option<String> = row.get(4)?;
            let link: Option<String> = row.get(5)?;
            let published_at: Option<i64> = row.get(6)?;
            let first_seen_at: i64 = row.get(7)?;
            let sort_key: i64 = row.get(8)?;
            entries.push(EntryListRow {
                internal_id: id,
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

    /// Loads tags grouped by entry id.
    pub fn load_tags(&self, entry_ids: &[i64]) -> Result<HashMap<i64, Vec<String>>, AppError> {
        if entry_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = std::iter::repeat_n("?", entry_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = q::load_tags_by_entry_ids(&placeholders);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(entry_ids.iter()))?;
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        while let Some(row) = rows.next()? {
            let entry_id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            tags.entry(entry_id).or_default().push(name);
        }
        Ok(tags)
    }

    /// Loads one entry detail row tuple for view operation.
    pub fn view_entry_row(&self, entry_id: &str) -> Result<Option<EntryDetailRow>, AppError> {
        self.conn
            .query_row(q::SELECT_ENTRY_DETAIL_BY_ID, params![entry_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })
            .optional()
            .map_err(AppError::from)
    }

    /// Loads content payload and content type for one entry.
    pub fn load_content(
        &self,
        data_dir: &Path,
        entry_id: i64,
    ) -> Result<(Option<String>, Option<String>), AppError> {
        let row = self
            .conn
            .query_row(
                q::SELECT_ENTRY_CONTENT_BY_ENTRY_ID,
                params![entry_id],
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
                let content = fs::read_to_string(path).ok();
                Ok((content, content_type))
            }
            Storage::None => Ok((None, content_type)),
        }
    }

    /// Loads enclosures for one entry.
    pub fn load_enclosures(&self, entry_id: i64) -> Result<Vec<EntryEnclosure>, AppError> {
        let mut stmt = self.conn.prepare(q::SELECT_ENTRY_ENCLOSURES_BY_ENTRY_ID)?;
        let mut rows = stmt.query(params![entry_id])?;
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

    /// Resolves internal entry ids keyed by stable entry key.
    pub fn find_entry_ids_by_keys(
        &self,
        entry_keys: &[String],
    ) -> Result<HashMap<String, i64>, AppError> {
        EntryReadRepo::new(self.conn).find_entry_ids_by_keys(entry_keys)
    }

    /// Ensures all requested entry keys exist.
    pub fn ensure_all_entry_keys_exist(&self, entry_keys: &[String]) -> Result<(), AppError> {
        EntryReadRepo::new(self.conn).ensure_all_entry_keys_exist(entry_keys)
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
    pub fn insert_entry_tag(&self, entry_id: i64, tag_id: i64) -> Result<usize, AppError> {
        let rows = self
            .conn
            .execute(q::INSERT_ENTRY_TAG_IGNORE, params![entry_id, tag_id])?;
        Ok(rows)
    }

    /// Deletes one entry-tag relation.
    pub fn delete_entry_tag(&self, entry_id: i64, tag_id: i64) -> Result<usize, AppError> {
        let rows = self
            .conn
            .execute(q::DELETE_ENTRY_TAG, params![entry_id, tag_id])?;
        Ok(rows)
    }
}
