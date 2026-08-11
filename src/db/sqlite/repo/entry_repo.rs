//! Entry repositories for SQLite-backed listing and mutation operations.

use crate::db::EntryContentStorage as Storage;
use crate::db::sqlite::query::{entries as q, sql_placeholders};
use crate::db::sqlite::tags;
use crate::error::AppError;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use std::collections::{HashMap, HashSet};

/// Payload for the entry detail base row selected from SQLite.
pub(crate) struct EntryDetailRow {
    pub entry_pk: i64,
    pub entry_id: String,
    pub feed_id: String,
    pub feed_title: Option<String>,
    pub title: Option<String>,
    pub link: Option<String>,
    pub author: Option<String>,
    pub published_at: Option<i64>,
    pub first_seen_at: i64,
}

/// Content row selected from SQLite.
pub(crate) struct EntryContentRow {
    pub storage: Storage,
    pub reference: Option<String>,
    pub content_type: Option<String>,
    pub content: Option<String>,
}

/// Enclosure row selected from SQLite.
pub(crate) struct EntryEnclosureRow {
    pub url: String,
    pub mime_type: Option<String>,
    pub length: Option<i64>,
}

/// One list row with metadata required to finalize response payloads.
pub(crate) struct EntryListRow {
    /// Internal entry id used for joins and tag loading.
    pub entry_pk: i64,
    pub entry_id: String,
    pub feed_id: String,
    pub feed_title: Option<String>,
    pub title: Option<String>,
    pub link: Option<String>,
    pub published_at: Option<i64>,
    pub first_seen_at: i64,
}

/// SQL-free filter input for entry list queries.
#[derive(Debug, Clone)]
pub(crate) enum EntryListFilter {
    FeedPk(i64),
    FeedTitle(String),
    TitleContains(String),
    TagIds(Vec<i64>),
    Not(Box<Self>),
    And(Vec<Self>),
    Or(Vec<Self>),
    EffectiveDateAtLeast(i64),
    EffectiveDateBefore(i64),
    Cursor { key: i64, entry_pk: i64 },
}

/// SQL-free sort input for entry list queries.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EntryListSort {
    DateDesc,
    DateAsc,
    FirstSeenDesc,
    FirstSeenAsc,
}

fn entry_list_row_from_row(row: &Row<'_>) -> Result<EntryListRow, rusqlite::Error> {
    let entry_pk: i64 = row.get(0)?;
    let entry_id: String = row.get(1)?;
    let feed_id: String = row.get(2)?;
    let feed_title: Option<String> = row.get(3)?;
    let title: Option<String> = row.get(4)?;
    let link: Option<String> = row.get(5)?;
    let published_at: Option<i64> = row.get(6)?;
    let first_seen_at: i64 = row.get(7)?;
    Ok(EntryListRow {
        entry_pk,
        entry_id,
        feed_id,
        feed_title,
        title,
        link,
        published_at,
        first_seen_at,
    })
}

const TITLE_CONTAINS_CLAUSE: &str = r#"(e.title IS NOT NULL AND e.title LIKE ? ESCAPE '\')"#;

fn list_sort_key_expr(sort: EntryListSort) -> &'static str {
    match sort {
        EntryListSort::DateDesc | EntryListSort::DateAsc => q::EFFECTIVE_DATE_EXPR,
        EntryListSort::FirstSeenDesc | EntryListSort::FirstSeenAsc => "e.first_seen_at",
    }
}

fn list_order_clause(sort: EntryListSort) -> &'static str {
    match sort {
        EntryListSort::DateDesc => q::ORDER_BY_DATE_DESC,
        EntryListSort::DateAsc => q::ORDER_BY_DATE_ASC,
        EntryListSort::FirstSeenDesc => q::ORDER_BY_FIRST_SEEN_DESC,
        EntryListSort::FirstSeenAsc => q::ORDER_BY_FIRST_SEEN_ASC,
    }
}

fn list_where_clause(filters: &[EntryListFilter], sort: EntryListSort) -> (String, Vec<Value>) {
    let mut params = Vec::new();
    let clauses = filters
        .iter()
        .map(|filter| format!("({})", list_filter_clause(filter, sort, &mut params)))
        .collect::<Vec<_>>();
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("{}{}", q::WHERE_PREFIX, clauses.join(" AND "))
    };
    (where_sql, params)
}

fn list_filter_clause(
    filter: &EntryListFilter,
    sort: EntryListSort,
    params: &mut Vec<Value>,
) -> String {
    match filter {
        EntryListFilter::FeedPk(feed_pk) => {
            params.push(Value::from(*feed_pk));
            q::ENTRY_FEED_PK_EQ.to_string()
        }
        EntryListFilter::FeedTitle(title) => {
            params.push(Value::from(title.clone()));
            q::EXISTS_FEED_TITLE_FOR_ENTRY.to_string()
        }
        EntryListFilter::TitleContains(title) => {
            params.push(Value::from(like_contains_pattern(title)));
            TITLE_CONTAINS_CLAUSE.to_string()
        }
        EntryListFilter::TagIds(tag_ids) => match tag_ids.as_slice() {
            [] => "0=1".to_string(),
            [tag_id] => {
                params.push(Value::from(*tag_id));
                q::EXISTS_TAG_ID_FOR_ENTRY.to_string()
            }
            _ => {
                for tag_id in tag_ids {
                    params.push(Value::from(*tag_id));
                }
                let placeholders = sql_placeholders(tag_ids.len());
                q::exists_tag_ids_for_entry(&placeholders)
            }
        },
        EntryListFilter::Not(inner) => {
            format!("NOT ({})", list_filter_clause(inner, sort, params))
        }
        EntryListFilter::And(items) => join_list_filter_clauses(items, " AND ", sort, params),
        EntryListFilter::Or(items) => join_list_filter_clauses(items, " OR ", sort, params),
        EntryListFilter::EffectiveDateAtLeast(value) => {
            params.push(Value::from(*value));
            format!("({}) >= ?", q::EFFECTIVE_DATE_EXPR)
        }
        EntryListFilter::EffectiveDateBefore(value) => {
            params.push(Value::from(*value));
            format!("({}) < ?", q::EFFECTIVE_DATE_EXPR)
        }
        EntryListFilter::Cursor { key, entry_pk } => {
            params.push(Value::from(*key));
            params.push(Value::from(*entry_pk));
            let operator = match sort {
                EntryListSort::DateDesc | EntryListSort::FirstSeenDesc => "<",
                EntryListSort::DateAsc | EntryListSort::FirstSeenAsc => ">",
            };
            format!("({}, e.id) {operator} (?, ?)", list_sort_key_expr(sort))
        }
    }
}

fn join_list_filter_clauses(
    items: &[EntryListFilter],
    separator: &str,
    sort: EntryListSort,
    params: &mut Vec<Value>,
) -> String {
    items
        .iter()
        .map(|item| format!("({})", list_filter_clause(item, sort, params)))
        .collect::<Vec<_>>()
        .join(separator)
}

fn like_contains_pattern(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    format!("%{escaped}%")
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
            let placeholders = sql_placeholders(chunk.len());
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

    /// Lists `(entry_pk, sort_key)` tuples using the provided filters.
    pub(crate) fn list_filtered_entry_sort_keys(
        &self,
        filters: &[EntryListFilter],
        sort: EntryListSort,
    ) -> Result<Vec<(i64, i64)>, AppError> {
        let (where_sql, params) = list_where_clause(filters, sort);
        let sql = q::select_filtered_entry_sort_keys(&where_sql, list_sort_key_expr(sort));
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
            let placeholders = sql_placeholders(chunk.len());
            let sql = q::select_entry_rows_by_entry_pks(&placeholders);
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
            while let Some(row) = rows.next()? {
                rows_out.push(entry_list_row_from_row(row)?);
            }
        }
        Ok(rows_out)
    }

    /// Loads entry primary keys by tag ids in fixed-size chunks.
    pub fn find_entry_pks_by_tag_ids(
        &self,
        tag_ids: &[i64],
    ) -> Result<HashMap<i64, Vec<i64>>, AppError> {
        if tag_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut map: HashMap<i64, Vec<i64>> = HashMap::new();
        for tag_chunk in tag_ids.chunks(Self::IN_CHUNK_SIZE) {
            let tag_placeholders = sql_placeholders(tag_chunk.len());
            let sql = q::select_entry_pks_by_tag_ids(&tag_placeholders);
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(tag_chunk.iter()))?;
            while let Some(row) = rows.next()? {
                let tag_id: i64 = row.get(0)?;
                let entry_pk: i64 = row.get(1)?;
                map.entry(tag_id).or_default().push(entry_pk);
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

    /// Counts entries matching the provided filters.
    pub(crate) fn count_entries(
        &self,
        filters: &[EntryListFilter],
        sort: EntryListSort,
    ) -> Result<i64, AppError> {
        let (where_sql, params) = list_where_clause(filters, sort);
        let sql = q::count_entries(&where_sql);
        let total: i64 = self
            .conn
            .query_row(&sql, params_from_iter(params.iter()), |row| row.get(0))?;
        Ok(total)
    }

    /// Fetches one list page with the provided filters and sort.
    pub(crate) fn fetch_entries(
        &self,
        filters: &[EntryListFilter],
        sort: EntryListSort,
        limit: usize,
    ) -> Result<(Vec<EntryListRow>, Vec<i64>), AppError> {
        let fetch_limit = limit.saturating_add(1);
        let (where_sql, mut list_params) = list_where_clause(filters, sort);
        let sql = q::fetch_entries(
            &where_sql,
            list_sort_key_expr(sort),
            list_order_clause(sort),
        );
        list_params.push(Value::from(fetch_limit as i64));
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(list_params.iter()))?;
        let mut entries = Vec::new();
        let mut sort_keys = Vec::new();
        while let Some(row) = rows.next()? {
            let sort_key: i64 = row.get(8)?;
            entries.push(entry_list_row_from_row(row)?);
            sort_keys.push(sort_key);
        }
        Ok((entries, sort_keys))
    }

    /// Loads tags grouped by entry primary key.
    pub fn load_tags(&self, entry_pks: &[i64]) -> Result<HashMap<i64, Vec<String>>, AppError> {
        if entry_pks.is_empty() {
            return Ok(HashMap::new());
        }
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        for chunk in entry_pks.chunks(Self::IN_CHUNK_SIZE) {
            let placeholders = sql_placeholders(chunk.len());
            let sql = q::load_tags_by_entry_ids(&placeholders);
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
            while let Some(row) = rows.next()? {
                let entry_pk: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                tags.entry(entry_pk).or_default().push(name);
            }
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
        let placeholders = sql_placeholders(names.len());
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
    pub(crate) fn view_entry_row(
        &self,
        entry_id: &str,
    ) -> Result<Option<EntryDetailRow>, AppError> {
        self.conn
            .query_row(q::SELECT_ENTRY_DETAIL_BY_ID, params![entry_id], |row| {
                Ok(EntryDetailRow {
                    entry_pk: row.get(0)?,
                    entry_id: row.get(1)?,
                    feed_id: row.get(2)?,
                    feed_title: row.get(3)?,
                    title: row.get(4)?,
                    link: row.get(5)?,
                    author: row.get(6)?,
                    published_at: row.get(7)?,
                    first_seen_at: row.get(8)?,
                })
            })
            .optional()
            .map_err(AppError::from)
    }

    /// Loads and parses one entry content row.
    pub(crate) fn load_content_row(
        &self,
        entry_pk: i64,
    ) -> Result<Option<EntryContentRow>, AppError> {
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
            return Ok(None);
        };
        let storage = storage
            .parse::<Storage>()
            .map_err(|_| AppError::internal(format!("Unknown content storage: {storage}")))?;
        Ok(Some(EntryContentRow {
            storage,
            reference,
            content_type,
            content,
        }))
    }

    /// Finds persisted content references from the provided candidates.
    pub(crate) fn find_content_refs(
        &self,
        references: &[String],
    ) -> Result<HashSet<String>, AppError> {
        if references.is_empty() {
            return Ok(HashSet::new());
        }
        let mut found = HashSet::new();
        for chunk in references.chunks(Self::IN_CHUNK_SIZE) {
            let placeholders = sql_placeholders(chunk.len());
            let sql = q::select_content_refs_by_refs(&placeholders);
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
            while let Some(row) = rows.next()? {
                found.insert(row.get(0)?);
            }
        }
        Ok(found)
    }

    /// Loads enclosure rows for one entry.
    pub(crate) fn load_enclosure_rows(
        &self,
        entry_pk: i64,
    ) -> Result<Vec<EntryEnclosureRow>, AppError> {
        let mut stmt = self.conn.prepare(q::SELECT_ENTRY_ENCLOSURES_BY_ENTRY_ID)?;
        let mut rows = stmt.query(params![entry_pk])?;
        let mut enclosures = Vec::new();
        while let Some(row) = rows.next()? {
            enclosures.push(EntryEnclosureRow {
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
    const STAGE_CHUNK_SIZE: usize = 128;

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

    /// Clears temp tables used by bulk mark operations.
    pub fn clear_mark_temp_tables(&self) -> Result<(), AppError> {
        self.conn.execute_batch(q::CREATE_MARK_TEMP_TABLES)?;
        self.conn.execute_batch(q::CLEAR_MARK_TEMP_TABLES)?;
        Ok(())
    }

    /// Stages entry primary keys for a bulk mark operation.
    pub fn stage_mark_entry_pks(&self, entry_pks: &[i64]) -> Result<(), AppError> {
        self.stage_mark_ids(entry_pks, q::insert_temp_mark_entry_pks)
    }

    /// Stages tag ids for bulk mark add operation.
    pub fn stage_mark_add_tag_ids(&self, tag_ids: &[i64]) -> Result<(), AppError> {
        self.stage_mark_ids(tag_ids, q::insert_temp_mark_add_tag_ids)
    }

    /// Stages tag ids for bulk mark remove operation.
    pub fn stage_mark_remove_tag_ids(&self, tag_ids: &[i64]) -> Result<(), AppError> {
        self.stage_mark_ids(tag_ids, q::insert_temp_mark_remove_tag_ids)
    }

    /// Counts distinct entries whose tag relations would change.
    pub fn count_mark_changed_entries(&self) -> Result<usize, AppError> {
        let changed: i64 = self
            .conn
            .query_row(q::COUNT_MARK_CHANGED_ENTRIES, [], |row| row.get(0))?;
        Ok(changed as usize)
    }

    /// Applies staged mark add relations in bulk.
    pub fn apply_mark_adds(&self) -> Result<usize, AppError> {
        Ok(self.conn.execute(q::APPLY_MARK_ADDS, [])?)
    }

    /// Applies staged mark remove relations in bulk.
    pub fn apply_mark_removes(&self) -> Result<usize, AppError> {
        Ok(self.conn.execute(q::APPLY_MARK_REMOVES, [])?)
    }

    fn stage_mark_ids<F>(&self, ids: &[i64], sql_builder: F) -> Result<(), AppError>
    where
        F: Fn(&str) -> String,
    {
        if ids.is_empty() {
            return Ok(());
        }
        for chunk in ids.chunks(Self::STAGE_CHUNK_SIZE) {
            let placeholders = std::iter::repeat_n("(?)", chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = sql_builder(&placeholders);
            self.conn.execute(&sql, params_from_iter(chunk.iter()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{EntryReadRepo, EntryWriteRepo};
    use crate::db::FeedInput;
    use crate::db::migrate;
    use crate::db::sqlite::feeds::upsert_feed_with_conn;
    use rusqlite::{Connection, params};

    fn create_store_schema(conn: &Connection) {
        migrate::migrate(conn).expect("migrate schema");
    }

    fn insert_feed(conn: &Connection) {
        upsert_feed_with_conn(
            conn,
            &FeedInput {
                feed_id: "feed-1".to_string(),
                url: "https://example.com/feed".to_string(),
                title: Some("Feed Title".to_string()),
                author: None,
                site_url: None,
                meta_json: None,
            },
            1,
        )
        .expect("insert feed");
    }

    fn insert_entry(conn: &Connection, id: i64, entry_id: &str) {
        conn.execute(
            "INSERT INTO entries (id, entry_id, feed_pk, title, first_seen_at) VALUES (?1, ?2, 1, ?3, 1704067200)",
            params![id, entry_id, format!("Title {id}")],
        )
        .expect("insert entry");
    }

    fn insert_tag(conn: &Connection, id: i64, name: &str) {
        conn.execute(
            "INSERT INTO tags (id, name) VALUES (?1, ?2)",
            params![id, name],
        )
        .expect("insert tag");
    }

    fn count_entry_tag(conn: &Connection, entry_pk: i64, tag_id: i64) -> i64 {
        conn.query_row(
            "SELECT COUNT(1) FROM entry_tags WHERE entry_pk = ?1 AND tag_id = ?2",
            params![entry_pk, tag_id],
            |row| row.get(0),
        )
        .expect("count entry tag")
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

        assert_eq!(row.entry_pk, 42);
        assert_eq!(row.entry_id, "entry-1");
        assert_eq!(row.feed_id, "feed-1");
        assert_eq!(row.feed_title.as_deref(), Some("Feed Title"));
        assert_eq!(row.title.as_deref(), Some("Entry Title"));
    }

    #[test]
    fn staged_mark_adds_insert_only_missing_relations() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_store_schema(&conn);
        insert_feed(&conn);
        insert_entry(&conn, 1, "entry-1");
        insert_entry(&conn, 2, "entry-2");
        insert_tag(&conn, 10, "foo");
        conn.execute(
            "INSERT INTO entry_tags (entry_pk, tag_id) VALUES (1, 10)",
            [],
        )
        .expect("seed entry_tag");

        let repo = EntryWriteRepo::new(&conn);
        repo.clear_mark_temp_tables().expect("clear temp tables");
        repo.stage_mark_entry_pks(&[1, 2]).expect("stage entry pks");
        repo.stage_mark_add_tag_ids(&[10])
            .expect("stage add tag ids");

        let changed = repo
            .count_mark_changed_entries()
            .expect("count changed entries");
        assert_eq!(changed, 1);

        let inserted = repo.apply_mark_adds().expect("apply mark adds");
        assert_eq!(inserted, 1);
        assert_eq!(count_entry_tag(&conn, 1, 10), 1);
        assert_eq!(count_entry_tag(&conn, 2, 10), 1);
    }

    #[test]
    fn staged_mark_removes_delete_only_existing_relations() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_store_schema(&conn);
        insert_feed(&conn);
        insert_entry(&conn, 1, "entry-1");
        insert_entry(&conn, 2, "entry-2");
        insert_tag(&conn, 10, "foo");
        conn.execute(
            "INSERT INTO entry_tags (entry_pk, tag_id) VALUES (1, 10), (2, 10)",
            [],
        )
        .expect("seed entry tags");

        let repo = EntryWriteRepo::new(&conn);
        repo.clear_mark_temp_tables().expect("clear temp tables");
        repo.stage_mark_entry_pks(&[1, 3]).expect("stage entry pks");
        repo.stage_mark_remove_tag_ids(&[10])
            .expect("stage remove tag ids");

        let changed = repo
            .count_mark_changed_entries()
            .expect("count changed entries");
        assert_eq!(changed, 1);

        let deleted = repo.apply_mark_removes().expect("apply mark removes");
        assert_eq!(deleted, 1);
        assert_eq!(count_entry_tag(&conn, 1, 10), 0);
        assert_eq!(count_entry_tag(&conn, 2, 10), 1);
    }

    #[test]
    fn count_mark_changed_entries_deduplicates_entries_across_adds_and_removes() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_store_schema(&conn);
        insert_feed(&conn);
        insert_entry(&conn, 1, "entry-1");
        insert_entry(&conn, 2, "entry-2");
        insert_tag(&conn, 10, "foo");
        insert_tag(&conn, 20, "bar");
        conn.execute(
            "INSERT INTO entry_tags (entry_pk, tag_id) VALUES (1, 20), (2, 20)",
            [],
        )
        .expect("seed remove relations");

        let repo = EntryWriteRepo::new(&conn);
        repo.clear_mark_temp_tables().expect("clear temp tables");
        repo.stage_mark_entry_pks(&[1, 2]).expect("stage entry pks");
        repo.stage_mark_add_tag_ids(&[10]).expect("stage add ids");
        repo.stage_mark_remove_tag_ids(&[20])
            .expect("stage remove ids");

        let changed = repo
            .count_mark_changed_entries()
            .expect("count changed entries");
        assert_eq!(changed, 2);
    }

    #[test]
    fn clear_mark_temp_tables_removes_previous_stage_rows() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_store_schema(&conn);
        insert_feed(&conn);
        insert_entry(&conn, 1, "entry-1");
        insert_tag(&conn, 10, "foo");

        let repo = EntryWriteRepo::new(&conn);
        repo.clear_mark_temp_tables().expect("clear temp tables");
        repo.stage_mark_entry_pks(&[1]).expect("stage entry pks");
        repo.stage_mark_add_tag_ids(&[10]).expect("stage add ids");
        let changed = repo
            .count_mark_changed_entries()
            .expect("count changed entries");
        assert_eq!(changed, 1);

        repo.clear_mark_temp_tables().expect("clear temp tables");
        let changed_after_clear = repo
            .count_mark_changed_entries()
            .expect("count changed after clear");
        assert_eq!(changed_after_clear, 0);
    }

    #[test]
    fn find_entry_pks_by_tag_ids_returns_sorted_vectors() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        create_store_schema(&conn);
        insert_feed(&conn);
        insert_entry(&conn, 1, "entry-1");
        insert_entry(&conn, 2, "entry-2");
        insert_entry(&conn, 3, "entry-3");
        insert_tag(&conn, 10, "foo");
        insert_tag(&conn, 20, "bar");
        conn.execute(
            "INSERT INTO entry_tags (entry_pk, tag_id) VALUES (3, 10), (1, 10), (2, 20), (1, 20)",
            [],
        )
        .expect("seed entry tags");

        let map = EntryReadRepo::new(&conn)
            .find_entry_pks_by_tag_ids(&[10, 20])
            .expect("find entry pks by tag ids");

        assert_eq!(map.get(&10), Some(&vec![1, 3]));
        assert_eq!(map.get(&20), Some(&vec![1, 2]));
    }
}
