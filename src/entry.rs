//! Entry list/view/mark operations.

use crate::cli::SortOrder;
use crate::config::AppConfig;
use crate::content_ref;
use crate::db::sqlite::{SqliteStore, ensure_tag_with_conn};
use crate::db::{EntryContentStorage, EntryContentStorage as Storage};
use crate::error::AppError;
use crate::query::{EntryQuery, FeedFilter, TagExpr};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Entry summary for list responses.
#[derive(Debug, Serialize)]
pub struct EntrySummary {
    /// Entry id.
    pub id: i64,
    /// Feed id.
    pub feed_id: i64,
    /// Entry title.
    pub title: Option<String>,
    /// Entry link.
    pub link: Option<String>,
    /// Published time.
    pub published_at: Option<i64>,
    /// First seen time.
    pub first_seen_at: i64,
    /// Tags applied to the entry.
    pub tags: Vec<String>,
}

/// Entry enclosure payload.
#[derive(Debug, Serialize)]
pub struct EntryEnclosure {
    /// Enclosure URL.
    pub url: String,
    /// MIME type.
    pub mime_type: Option<String>,
    /// Length in bytes.
    pub length: Option<i64>,
}

/// Entry detail payload.
#[derive(Debug, Serialize)]
pub struct EntryDetail {
    /// Entry id.
    pub id: i64,
    /// Feed id.
    pub feed_id: i64,
    /// Feed title.
    pub feed_title: Option<String>,
    /// Entry title.
    pub title: Option<String>,
    /// Entry link.
    pub link: Option<String>,
    /// Entry author.
    pub author: Option<String>,
    /// Published time.
    pub published_at: Option<i64>,
    /// First seen time.
    pub first_seen_at: i64,
    /// Entry content body.
    pub content: Option<String>,
    /// Content type.
    pub content_type: Option<String>,
    /// Tags applied to the entry.
    pub tags: Vec<String>,
    /// Enclosure list.
    pub enclosures: Vec<EntryEnclosure>,
}

/// Entry list response payload.
#[derive(Debug, Serialize)]
pub struct EntryListResponse {
    /// Total hits for the query.
    pub total_count: i64,
    /// Page items.
    pub items: Vec<EntrySummary>,
    /// Cursor for the next page.
    pub next_page_token: Option<String>,
    /// Revision captured when the list was fetched.
    pub revision: i64,
    /// Write timestamp captured when the list was fetched.
    pub last_write_at: Option<i64>,
}

/// Cursor payload for pagination.
#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    k: i64,
    id: i64,
    sort: String,
    query_hash: String,
}

/// Lists entries using tag filters and cursor pagination.
pub fn list_entries(
    store: &SqliteStore,
    query: &EntryQuery,
    sort: SortOrder,
    limit: usize,
    cursor: Option<&str>,
) -> Result<EntryListResponse, AppError> {
    let conn = store.connection();
    let system_meta = store.read_system_meta()?;
    let query_hash = compute_query_hash(query);
    let (count_where_sql, count_params) = build_where_clause(query, sort, None, &query_hash)?;
    let total_count = count_entries(conn, &count_where_sql, &count_params)?;
    let (page_where_sql, page_params) = build_where_clause(query, sort, cursor, &query_hash)?;
    let (items, next_page_token) = fetch_entries(
        conn,
        &page_where_sql,
        &page_params,
        sort,
        limit,
        &query_hash,
    )?;
    Ok(EntryListResponse {
        total_count,
        items,
        next_page_token,
        revision: system_meta.revision,
        last_write_at: system_meta.updated_at,
    })
}

/// Loads entry detail by id.
pub fn view_entry(
    store: &SqliteStore,
    config: &AppConfig,
    entry_id: i64,
) -> Result<EntryDetail, AppError> {
    let conn = store.connection();
    let row = conn
        .query_row(
            "SELECT e.id, e.feed_id, f.title, e.title, e.link, e.author, e.published_at, e.first_seen_at
             FROM entries e JOIN feeds f ON e.feed_id = f.id WHERE e.id = ?1",
            params![entry_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;
    let (id, feed_id, feed_title, title, link, author, published_at, first_seen_at) =
        row.ok_or_else(|| AppError::entry_not_found(format!("Entry {entry_id} not found")))?;

    let tags = load_tags(conn, &[id])?.remove(&id).unwrap_or_default();
    let (content, content_type) = load_content(conn, &config.storage.data_dir, id)?;
    let enclosures = load_enclosures(conn, id)?;

    Ok(EntryDetail {
        id,
        feed_id,
        feed_title,
        title,
        link,
        author,
        published_at,
        first_seen_at,
        content,
        content_type,
        tags,
        enclosures,
    })
}

/// Updates entry tags and returns the number of affected entries.
pub fn mark_entries(
    store: &mut SqliteStore,
    entry_ids: &[i64],
    add_tags: &[String],
    remove_tags: &[String],
) -> Result<usize, AppError> {
    if add_tags.is_empty() && remove_tags.is_empty() {
        return Err(AppError::invalid_query(
            "mark tag requires --add or --remove",
        ));
    }
    let mut unique_ids = Vec::new();
    let mut seen = HashSet::new();
    for id in entry_ids {
        if seen.insert(*id) {
            unique_ids.push(*id);
        }
    }
    if unique_ids.is_empty() {
        return Ok(0);
    }
    let tx = store.transaction()?;
    let add_ids = ensure_tag_ids(&tx, add_tags)?;
    let remove_ids = lookup_tag_ids(&tx, remove_tags)?;
    let mut updated = 0usize;
    for entry_id in unique_ids {
        let mut changed = false;
        for tag_id in add_ids.values() {
            let rows = tx.execute(
                "INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?1, ?2)",
                params![entry_id, tag_id],
            )?;
            if rows > 0 {
                changed = true;
            }
        }
        for tag_id in remove_ids.values() {
            let rows = tx.execute(
                "DELETE FROM entry_tags WHERE entry_id = ?1 AND tag_id = ?2",
                params![entry_id, tag_id],
            )?;
            if rows > 0 {
                changed = true;
            }
        }
        if changed {
            updated += 1;
        }
    }
    tx.commit()?;
    Ok(updated)
}

fn build_where_clause(
    query: &EntryQuery,
    sort: SortOrder,
    cursor: Option<&str>,
    query_hash: &str,
) -> Result<(String, Vec<Value>), AppError> {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    if let Some(tag_expr) = &query.tag_expr {
        clauses.push(format!(
            "({})",
            build_tag_expr_clause(tag_expr, &mut params)
        ));
    }
    if let Some(feed) = &query.feed {
        match feed {
            FeedFilter::Id(id) => {
                clauses.push("e.feed_id = ?".to_string());
                params.push(Value::from(*id));
            }
            FeedFilter::Title(title) => {
                clauses.push(
                    "EXISTS (SELECT 1 FROM feeds f WHERE f.id = e.feed_id AND f.title = ?)"
                        .to_string(),
                );
                params.push(Value::from(title.clone()));
            }
        }
    }
    if let Some(title) = &query.title {
        clauses.push("e.title LIKE ?".to_string());
        params.push(Value::from(format!("%{title}%")));
    }
    if let Some(after) = query.after {
        clauses.push(format!("({}) >= ?", effective_date_expr()));
        params.push(Value::from(after));
    }
    if let Some(before) = query.before {
        clauses.push(format!("({}) < ?", effective_date_expr()));
        params.push(Value::from(before));
    }
    if let Some(cursor) = cursor {
        let cursor = decode_cursor(cursor, sort, query_hash)?;
        let key_expr = sort_key_expr(sort);
        let predicate = match sort {
            SortOrder::DateDesc | SortOrder::FirstSeenDesc => {
                format!("({key_expr}, e.id) < (? , ?)")
            }
            SortOrder::DateAsc | SortOrder::FirstSeenAsc => {
                format!("({key_expr}, e.id) > (? , ?)")
            }
        };
        clauses.push(predicate);
        params.push(Value::from(cursor.k));
        params.push(Value::from(cursor.id));
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    Ok((where_sql, params))
}

fn count_entries(conn: &Connection, where_sql: &str, params: &[Value]) -> Result<i64, AppError> {
    let sql = format!("SELECT COUNT(1) FROM entries e {where_sql}");
    let total: i64 = conn.query_row(&sql, params_from_iter(params.iter()), |row| row.get(0))?;
    Ok(total)
}

fn fetch_entries(
    conn: &Connection,
    where_sql: &str,
    params: &[Value],
    sort: SortOrder,
    limit: usize,
    query_hash: &str,
) -> Result<(Vec<EntrySummary>, Option<String>), AppError> {
    let key_expr = sort_key_expr(sort);
    let order_clause = sort_order_clause(sort);
    let fetch_limit = limit.saturating_add(1);
    let sql = format!(
        "SELECT e.id, e.feed_id, e.title, e.link, e.published_at, e.first_seen_at, {key_expr} AS sort_key \
         FROM entries e {where_sql} ORDER BY {order_clause} LIMIT ?"
    );
    let mut list_params = params.to_vec();
    list_params.push(Value::from(fetch_limit as i64));
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(list_params.iter()))?;
    let mut entries = Vec::new();
    let mut sort_keys = Vec::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let feed_id: i64 = row.get(1)?;
        let title: Option<String> = row.get(2)?;
        let link: Option<String> = row.get(3)?;
        let published_at: Option<i64> = row.get(4)?;
        let first_seen_at: i64 = row.get(5)?;
        let sort_key: i64 = row.get(6)?;
        entries.push(EntrySummary {
            id,
            feed_id,
            title,
            link,
            published_at,
            first_seen_at,
            tags: Vec::new(),
        });
        sort_keys.push(sort_key);
    }
    let has_next = entries.len() > limit;
    if has_next {
        entries.truncate(limit);
        sort_keys.truncate(limit);
    }
    let ids: Vec<i64> = entries.iter().map(|entry| entry.id).collect();
    let tags = load_tags(conn, &ids)?;
    for entry in &mut entries {
        entry.tags = tags.get(&entry.id).cloned().unwrap_or_default();
    }
    let next_page_token = if has_next {
        match (entries.last(), sort_keys.last()) {
            (Some(entry), Some(key)) => {
                Some(encode_cursor_with_query(*key, entry.id, sort, query_hash)?)
            }
            _ => None,
        }
    } else {
        None
    };
    Ok((entries, next_page_token))
}

fn sort_key_expr(sort: SortOrder) -> &'static str {
    match sort {
        SortOrder::DateDesc | SortOrder::DateAsc => effective_date_expr(),
        SortOrder::FirstSeenDesc | SortOrder::FirstSeenAsc => "e.first_seen_at",
    }
}

/// Returns the SQL expression for effective date filtering.
fn effective_date_expr() -> &'static str {
    "COALESCE(e.published_at, e.updated_at, e.first_seen_at)"
}

fn sort_order_clause(sort: SortOrder) -> &'static str {
    match sort {
        SortOrder::DateDesc => {
            "COALESCE(e.published_at, e.updated_at, e.first_seen_at) DESC, e.id DESC"
        }
        SortOrder::DateAsc => {
            "COALESCE(e.published_at, e.updated_at, e.first_seen_at) ASC, e.id ASC"
        }
        SortOrder::FirstSeenDesc => "e.first_seen_at DESC, e.id DESC",
        SortOrder::FirstSeenAsc => "e.first_seen_at ASC, e.id ASC",
    }
}

fn load_tags(conn: &Connection, entry_ids: &[i64]) -> Result<HashMap<i64, Vec<String>>, AppError> {
    if entry_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat_n("?", entry_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT et.entry_id, t.name FROM entry_tags et JOIN tags t ON et.tag_id = t.id \
         WHERE et.entry_id IN ({placeholders}) ORDER BY t.name"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(entry_ids.iter()))?;
    let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
    while let Some(row) = rows.next()? {
        let entry_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        tags.entry(entry_id).or_default().push(name);
    }
    Ok(tags)
}

fn load_content(
    conn: &Connection,
    data_dir: &Path,
    entry_id: i64,
) -> Result<(Option<String>, Option<String>), AppError> {
    let row = conn
        .query_row(
            "SELECT storage, ref, content_type, content FROM entry_contents WHERE entry_id = ?1",
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
    let storage = EntryContentStorage::from_str(&storage)
        .ok_or_else(|| AppError::internal(format!("Unknown content storage: {storage}")))?;
    match storage {
        Storage::Db => Ok((content, content_type)),
        Storage::Fs => {
            let Some(reference) = reference else {
                return Ok((None, content_type));
            };
            let path = match content_ref::sha256_path(data_dir, &reference) {
                Ok(path) => path,
                Err(_) => return Ok((None, content_type)),
            };
            let content = fs::read_to_string(path).ok();
            Ok((content, content_type))
        }
        Storage::None => Ok((None, content_type)),
    }
}

fn load_enclosures(conn: &Connection, entry_id: i64) -> Result<Vec<EntryEnclosure>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT url, mime_type, length FROM entry_enclosures WHERE entry_id = ?1 ORDER BY id",
    )?;
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

fn ensure_tag_ids(conn: &Connection, tags: &[String]) -> Result<HashMap<String, i64>, AppError> {
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for tag in tags {
        if seen.insert(tag.clone()) {
            unique.push(tag.clone());
        }
    }
    for tag in &unique {
        ensure_tag_with_conn(conn, tag)?;
    }
    lookup_tag_ids(conn, &unique)
}

fn lookup_tag_ids(conn: &Connection, tags: &[String]) -> Result<HashMap<String, i64>, AppError> {
    if tags.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat_n("?", tags.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT id, name FROM tags WHERE name IN ({placeholders})");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(tags.iter()))?;
    let mut map = HashMap::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        map.insert(name, id);
    }
    Ok(map)
}

/// Encodes pagination cursor with query metadata.
fn encode_cursor_with_query(
    key: i64,
    id: i64,
    sort: SortOrder,
    query_hash: &str,
) -> Result<String, AppError> {
    let cursor = Cursor {
        k: key,
        id,
        sort: sort.as_str().to_string(),
        query_hash: query_hash.to_string(),
    };
    let bytes = serde_json::to_vec(&cursor)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Decodes and validates pagination cursor.
fn decode_cursor(raw: &str, sort: SortOrder, query_hash: &str) -> Result<Cursor, AppError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(raw.as_bytes())
        .map_err(|error| AppError::invalid_query(format!("Invalid cursor: {error}")))?;
    let cursor: Cursor = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::invalid_query(format!("Invalid cursor: {error}")))?;
    if cursor.sort != sort.as_str() || cursor.query_hash != query_hash {
        return Err(AppError::invalid_query(
            "Cursor does not match the current query",
        ));
    }
    Ok(cursor)
}

/// Computes a stable hash for query validation.
fn compute_query_hash(query: &EntryQuery) -> String {
    let mut components = Vec::new();
    if let Some(tag_expr) = &query.tag_expr {
        components.push(format!("tag_expr={}", tag_expr.canonical()));
    }
    if let Some(feed) = &query.feed {
        match feed {
            FeedFilter::Id(id) => components.push(format!("feed_id={id}")),
            FeedFilter::Title(title) => components.push(format!("feed_title={title}")),
        }
    }
    if let Some(title) = &query.title {
        components.push(format!("title={title}"));
    }
    if let Some(after) = query.after {
        components.push(format!("after={after}"));
    }
    if let Some(before) = query.before {
        components.push(format!("before={before}"));
    }
    let payload = components.join("|");
    let mut hasher = Sha1::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

/// Builds SQL for a tag expression and appends bind params.
fn build_tag_expr_clause(expr: &TagExpr, params: &mut Vec<Value>) -> String {
    match expr {
        TagExpr::Tag(tag) => {
            params.push(Value::from(tag.clone()));
            "EXISTS (SELECT 1 FROM entry_tags et JOIN tags t ON et.tag_id = t.id \
             WHERE et.entry_id = e.id AND t.name = ?)"
                .to_string()
        }
        TagExpr::Not(inner) => format!("NOT ({})", build_tag_expr_clause(inner, params)),
        TagExpr::And(items) => {
            let clauses = items
                .iter()
                .map(|item| format!("({})", build_tag_expr_clause(item, params)))
                .collect::<Vec<_>>();
            clauses.join(" AND ")
        }
        TagExpr::Or(items) => {
            let clauses = items
                .iter()
                .map(|item| format!("({})", build_tag_expr_clause(item, params)))
                .collect::<Vec<_>>();
            clauses.join(" OR ")
        }
    }
}
