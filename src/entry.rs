//! Entry list/view/mark operations.

use crate::cli::SortOrder;
use crate::config::AppConfig;
use crate::db::sqlite::SqliteStore;
use crate::db::sqlite::query::entries as q;
use crate::db::sqlite::repo::EntryReadRepo;
use crate::error::AppError;
use crate::query::{EntryQuery, FeedFilter, TagExpr};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::types::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashSet};

/// Entry summary for list responses.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntrySummary {
    /// Entry id.
    pub entry_id: String,
    /// Feed id.
    pub feed_id: String,
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
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryEnclosure {
    /// Enclosure URL.
    pub url: String,
    /// MIME type.
    pub mime_type: Option<String>,
    /// Length in bytes.
    pub length: Option<i64>,
}

/// Entry detail payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryDetail {
    /// Entry id.
    pub entry_id: String,
    /// Feed id.
    pub feed_id: String,
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
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryListResponse {
    /// Total hits for the query.
    pub total_count: i64,
    /// Page items.
    pub items: Vec<EntrySummary>,
    /// Feed dictionary for feed id to title mapping.
    pub feeds: Vec<FeedSummary>,
    /// Cursor for the next page.
    pub next_page_token: Option<String>,
    /// Revision captured when the list was fetched.
    pub revision: i64,
    /// Write timestamp captured when the list was fetched.
    pub last_write_at: Option<i64>,
}

/// Feed dictionary item used by entry list responses.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeedSummary {
    /// Feed id.
    pub feed_id: String,
    /// Feed title.
    pub title: Option<String>,
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
    let entry_repo = store.entry_read_repo();
    let system_meta = store.sync_read_repo().read_system_meta()?;
    let query_hash = compute_query_hash(query);
    let (count_where_sql, count_params) = build_where_clause(query, sort, None, &query_hash)?;
    let total_count = entry_repo.count_entries(&count_where_sql, &count_params)?;
    let (page_where_sql, page_params) = build_where_clause(query, sort, cursor, &query_hash)?;
    let (items, feeds, next_page_token) = fetch_entries(
        &entry_repo,
        &page_where_sql,
        &page_params,
        sort,
        limit,
        &query_hash,
    )?;
    Ok(EntryListResponse {
        total_count,
        items,
        feeds,
        next_page_token,
        revision: system_meta.revision,
        last_write_at: system_meta.updated_at,
    })
}

/// Loads entry detail by id.
pub fn view_entry(
    store: &SqliteStore,
    config: &AppConfig,
    entry_id: &str,
) -> Result<EntryDetail, AppError> {
    let entry_repo = store.entry_read_repo();
    let row = entry_repo.view_entry_row(entry_id)?;
    let (entry_id, feed_id, feed_title, title, link, author, published_at, first_seen_at) = row
        .ok_or_else(|| {
            AppError::entry_not_found_with_details(
                format!("Entry {entry_id} not found"),
                json!({
                    "resource": "entry",
                    "entry_id": entry_id
                }),
            )
        })?;

    let entry_pks = entry_repo.find_entry_ids_by_keys(&[entry_id.clone()])?;
    let entry_pk = entry_pks.get(&entry_id).copied().ok_or_else(|| {
        AppError::entry_not_found_with_details(
            format!("Entry {entry_id} not found"),
            json!({
                "resource": "entry",
                "entry_id": entry_id
            }),
        )
    })?;
    let tags = entry_repo
        .load_tags(&[entry_pk])?
        .remove(&entry_pk)
        .unwrap_or_default();
    let (content, content_type) = entry_repo.load_content(&config.storage.data_dir, entry_pk)?;
    let enclosures = entry_repo.load_enclosures(entry_pk)?;

    Ok(EntryDetail {
        entry_id,
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
///
/// Returns `ENTRY_NOT_FOUND` when any requested entry id does not exist.
pub fn mark_entries(
    store: &mut SqliteStore,
    entry_ids: &[String],
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
        if seen.insert(id.clone()) {
            unique_ids.push(id.clone());
        }
    }
    if unique_ids.is_empty() {
        return Ok(0);
    }
    let tx = store.tx()?;
    let tx_entry_repo = tx.entry_write_repo();
    tx_entry_repo.ensure_all_entry_keys_exist(&unique_ids)?;
    let entry_pks = tx_entry_repo.find_entry_ids_by_keys(&unique_ids)?;
    let add_ids = tx_entry_repo.ensure_tag_ids(add_tags)?;
    let remove_ids = tx_entry_repo.lookup_tag_ids(remove_tags)?;
    let mut updated = 0usize;
    for entry_id in unique_ids {
        let Some(entry_pk) = entry_pks.get(&entry_id).copied() else {
            continue;
        };
        let mut changed = false;
        for tag_id in add_ids.values() {
            let rows = tx_entry_repo.insert_entry_tag(entry_pk, *tag_id)?;
            if rows > 0 {
                changed = true;
            }
        }
        for tag_id in remove_ids.values() {
            let rows = tx_entry_repo.delete_entry_tag(entry_pk, *tag_id)?;
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
                clauses.push(q::EXISTS_FEED_KEY_FOR_ENTRY.to_string());
                params.push(Value::from(id.clone()));
            }
            FeedFilter::Title(title) => {
                clauses.push(q::EXISTS_FEED_TITLE_FOR_ENTRY.to_string());
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
        format!("{}{}", q::WHERE_PREFIX, clauses.join(" AND "))
    };
    Ok((where_sql, params))
}

fn fetch_entries(
    entry_repo: &EntryReadRepo<'_>,
    where_sql: &str,
    params: &[Value],
    sort: SortOrder,
    limit: usize,
    query_hash: &str,
) -> Result<(Vec<EntrySummary>, Vec<FeedSummary>, Option<String>), AppError> {
    let key_expr = sort_key_expr(sort);
    let order_clause = sort_order_clause(sort);
    let (mut rows, mut sort_keys) =
        entry_repo.fetch_entries(where_sql, params, key_expr, order_clause, limit)?;
    let has_next = rows.len() > limit;
    if has_next {
        rows.truncate(limit);
        sort_keys.truncate(limit);
    }
    let ids: Vec<i64> = rows.iter().map(|entry| entry.entry_pk).collect();
    let tags = entry_repo.load_tags(&ids)?;
    for row in &mut rows {
        row.summary.tags = tags.get(&row.entry_pk).cloned().unwrap_or_default();
    }
    let feeds = rows
        .iter()
        .fold(BTreeMap::<String, Option<String>>::new(), |mut map, row| {
            map.entry(row.summary.feed_id.clone())
                .or_insert_with(|| row.feed_title.clone());
            map
        })
        .into_iter()
        .map(|(feed_id, title)| FeedSummary { feed_id, title })
        .collect::<Vec<_>>();
    let entries = rows.into_iter().map(|row| row.summary).collect::<Vec<_>>();
    let next_page_token = if has_next {
        match (ids.last(), sort_keys.last()) {
            (Some(id), Some(key)) => Some(encode_cursor_with_query(*key, *id, sort, query_hash)?),
            _ => None,
        }
    } else {
        None
    };
    Ok((entries, feeds, next_page_token))
}

fn sort_key_expr(sort: SortOrder) -> &'static str {
    match sort {
        SortOrder::DateDesc | SortOrder::DateAsc => effective_date_expr(),
        SortOrder::FirstSeenDesc | SortOrder::FirstSeenAsc => "e.first_seen_at",
    }
}

/// Returns the SQL expression for effective date filtering.
fn effective_date_expr() -> &'static str {
    q::EFFECTIVE_DATE_EXPR
}

fn sort_order_clause(sort: SortOrder) -> &'static str {
    match sort {
        SortOrder::DateDesc => q::ORDER_BY_DATE_DESC,
        SortOrder::DateAsc => q::ORDER_BY_DATE_ASC,
        SortOrder::FirstSeenDesc => q::ORDER_BY_FIRST_SEEN_DESC,
        SortOrder::FirstSeenAsc => q::ORDER_BY_FIRST_SEEN_ASC,
    }
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
    let bytes = URL_SAFE_NO_PAD.decode(raw.as_bytes()).map_err(|error| {
        AppError::invalid_query_with_details(
            format!("Invalid cursor: {error}"),
            json!({
                "kind": "invalid_cursor",
                "field": "cursor",
                "value": raw,
                "hint": "base64url_decode_failed"
            }),
        )
    })?;
    let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|error| {
        AppError::invalid_query_with_details(
            format!("Invalid cursor: {error}"),
            json!({
                "kind": "invalid_cursor",
                "field": "cursor",
                "value": raw,
                "hint": "cursor_json_decode_failed"
            }),
        )
    })?;
    if cursor.sort != sort.as_str() || cursor.query_hash != query_hash {
        return Err(AppError::invalid_query_with_details(
            "Cursor does not match the current query",
            json!({
                "kind": "invalid_cursor",
                "field": "cursor",
                "value": raw,
                "hint": "cursor_mismatch",
            }),
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
            q::EXISTS_TAG_FOR_ENTRY.to_string()
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
