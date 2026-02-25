//! Entry list/view/mark operations.

use crate::cli::SortOrder;
use crate::config::AppConfig;
use crate::db::sqlite::SqliteStore;
use crate::db::sqlite::query::entries as q;
use crate::db::sqlite::repo::EntryReadRepo;
use crate::error::{AppError, error_details};
use crate::query::{EntryQuery, FeedFilter, TagExpr};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::types::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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

type EntryListPage = (Vec<EntrySummary>, Vec<FeedSummary>, Option<String>);

enum FeedIdPredicate {
    NotRequested,
    Resolved(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagEvalPath {
    Simple,
    Complex,
}

const SIMPLE_PATH_MAX_NODE_COUNT: usize = 12;
const SIMPLE_PATH_MAX_DEPTH: usize = 4;
const SIMPLE_PATH_MAX_OR_FANOUT: usize = 6;
const ENTRY_PK_CHUNK_SIZE: usize = 500;

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
    let feed_id_predicate = resolve_feed_id_predicate(store, query)?;
    let resolved_tag_ids = resolve_tag_id_map(store, query)?;
    let (tag_clause, tag_params) = match &query.tag_expr {
        Some(tag_expr) => match route_tag_eval_path(tag_expr) {
            TagEvalPath::Simple => {
                let mut params = Vec::new();
                let clause = build_tag_expr_clause(tag_expr, &mut params, &resolved_tag_ids);
                (Some(clause), params)
            }
            TagEvalPath::Complex => {
                let matched_pks = resolve_complex_tag_entry_pks(
                    &entry_repo,
                    query,
                    sort,
                    &query_hash,
                    &feed_id_predicate,
                    &resolved_tag_ids,
                    tag_expr,
                )?;
                if matched_pks.is_empty() {
                    return Ok(EntryListResponse {
                        total_count: 0,
                        items: Vec::new(),
                        feeds: Vec::new(),
                        next_page_token: None,
                        revision: system_meta.revision,
                        last_write_at: system_meta.updated_at,
                    });
                }
                let (clause, params) = build_entry_pk_filter_clause(&matched_pks);
                (Some(clause), params)
            }
        },
        None => (None, Vec::new()),
    };
    let (count_where_sql, count_params) = build_where_clause(
        query,
        sort,
        None,
        &query_hash,
        &feed_id_predicate,
        tag_clause.as_deref(),
        &tag_params,
    )?;
    let total_count = entry_repo.count_entries(&count_where_sql, &count_params)?;
    let (page_where_sql, page_params) = build_where_clause(
        query,
        sort,
        cursor,
        &query_hash,
        &feed_id_predicate,
        tag_clause.as_deref(),
        &tag_params,
    )?;
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

fn route_tag_eval_path(expr: &TagExpr) -> TagEvalPath {
    if expr.contains_not()
        || expr.node_count() > SIMPLE_PATH_MAX_NODE_COUNT
        || expr.max_depth() > SIMPLE_PATH_MAX_DEPTH
        || expr.max_or_fanout() > SIMPLE_PATH_MAX_OR_FANOUT
    {
        TagEvalPath::Complex
    } else {
        TagEvalPath::Simple
    }
}

fn resolve_complex_tag_entry_pks(
    entry_repo: &EntryReadRepo<'_>,
    query: &EntryQuery,
    sort: SortOrder,
    query_hash: &str,
    feed_id_predicate: &FeedIdPredicate,
    resolved_tag_ids: &HashMap<String, i64>,
    tag_expr: &TagExpr,
) -> Result<Vec<i64>, AppError> {
    let (universe_where_sql, universe_params) =
        build_non_tag_where_clause(query, sort, None, query_hash, feed_id_predicate)?;
    let universe_pks = entry_repo.list_filtered_entry_pks(&universe_where_sql, &universe_params)?;
    if universe_pks.is_empty() {
        return Ok(Vec::new());
    }
    let universe = universe_pks.iter().copied().collect::<HashSet<_>>();
    let tag_ids = resolved_tag_ids.values().copied().collect::<Vec<_>>();
    let tag_entry_pks = entry_repo.find_entry_pks_by_tag_ids(&tag_ids)?;
    let matched = evaluate_tag_expr_set(tag_expr, &universe, resolved_tag_ids, &tag_entry_pks);
    let mut pks = matched.into_iter().collect::<Vec<_>>();
    pks.sort_unstable();
    Ok(pks)
}

fn evaluate_tag_expr_set(
    expr: &TagExpr,
    universe: &HashSet<i64>,
    resolved_tag_ids: &HashMap<String, i64>,
    tag_entry_pks: &HashMap<i64, HashSet<i64>>,
) -> HashSet<i64> {
    match expr {
        TagExpr::Tag(tag) => {
            let Some(tag_id) = resolved_tag_ids.get(tag) else {
                return HashSet::new();
            };
            let Some(entry_pks) = tag_entry_pks.get(tag_id) else {
                return HashSet::new();
            };
            entry_pks
                .iter()
                .filter(|pk| universe.contains(pk))
                .copied()
                .collect()
        }
        TagExpr::Not(inner) => {
            let inner_set = evaluate_tag_expr_set(inner, universe, resolved_tag_ids, tag_entry_pks);
            universe.difference(&inner_set).copied().collect()
        }
        TagExpr::And(items) => {
            let mut iter = items.iter();
            let Some(first) = iter.next() else {
                return HashSet::new();
            };
            let mut acc = evaluate_tag_expr_set(first, universe, resolved_tag_ids, tag_entry_pks);
            for item in iter {
                let next = evaluate_tag_expr_set(item, universe, resolved_tag_ids, tag_entry_pks);
                acc.retain(|pk| next.contains(pk));
                if acc.is_empty() {
                    break;
                }
            }
            acc
        }
        TagExpr::Or(items) => {
            let mut acc = HashSet::new();
            for item in items {
                let next = evaluate_tag_expr_set(item, universe, resolved_tag_ids, tag_entry_pks);
                acc.extend(next);
            }
            acc
        }
    }
}

fn resolve_feed_id_predicate(
    store: &SqliteStore,
    query: &EntryQuery,
) -> Result<FeedIdPredicate, AppError> {
    let Some(FeedFilter::Id(feed_id)) = &query.feed else {
        return Ok(FeedIdPredicate::NotRequested);
    };
    let resolved = store
        .feed_read_repo()
        .find_feed_pks_by_ids(std::slice::from_ref(feed_id))?;
    match resolved.get(feed_id) {
        Some(feed_pk) => Ok(FeedIdPredicate::Resolved(*feed_pk)),
        None => Err(AppError::entry_not_found_with_details(
            format!("Feed {feed_id} not found"),
            error_details([
                ("resource", JsonValue::from("feed")),
                ("feed_id", JsonValue::from(feed_id.clone())),
            ]),
        )),
    }
}

/// Collects all tag name literals referenced in an expression.
fn collect_tag_names(expr: &TagExpr, out: &mut BTreeSet<String>) {
    match expr {
        TagExpr::Tag(name) => {
            out.insert(name.clone());
        }
        TagExpr::Not(inner) => collect_tag_names(inner, out),
        TagExpr::And(items) | TagExpr::Or(items) => {
            for item in items {
                collect_tag_names(item, out);
            }
        }
    }
}

/// Resolves all tag names in the query to their database ids in one round-trip.
fn resolve_tag_id_map(
    store: &SqliteStore,
    query: &EntryQuery,
) -> Result<HashMap<String, i64>, AppError> {
    let Some(tag_expr) = &query.tag_expr else {
        return Ok(HashMap::new());
    };
    let mut names = BTreeSet::new();
    collect_tag_names(tag_expr, &mut names);
    let names_vec: Vec<String> = names.into_iter().collect();
    store.entry_read_repo().find_tag_ids_by_names(&names_vec)
}

/// Loads entry detail by id.
pub fn view_entry(
    store: &SqliteStore,
    config: &AppConfig,
    entry_id: &str,
) -> Result<EntryDetail, AppError> {
    let entry_repo = store.entry_read_repo();
    let row = entry_repo.view_entry_row(entry_id)?;
    let (entry_pk, entry_id, feed_id, feed_title, title, link, author, published_at, first_seen_at) =
        row.ok_or_else(|| {
            AppError::entry_not_found_with_details(
                format!("Entry {entry_id} not found"),
                error_details([
                    ("resource", JsonValue::from("entry")),
                    ("entry_id", JsonValue::from(entry_id.to_string())),
                ]),
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
    tx_entry_repo.ensure_all_entry_ids_exist(&unique_ids)?;
    let entry_pks = tx_entry_repo.find_entry_pks_by_ids(&unique_ids)?;
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
    feed_id_predicate: &FeedIdPredicate,
    extra_clause: Option<&str>,
    extra_params: &[Value],
) -> Result<(String, Vec<Value>), AppError> {
    let (mut clauses, mut params) =
        build_non_tag_predicates(query, sort, cursor, query_hash, feed_id_predicate)?;
    if let Some(extra_clause) = extra_clause {
        clauses.insert(0, format!("({extra_clause})"));
        let mut merged = extra_params.to_vec();
        merged.extend(params);
        params = merged;
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("{}{}", q::WHERE_PREFIX, clauses.join(" AND "))
    };
    Ok((where_sql, params))
}

fn build_non_tag_where_clause(
    query: &EntryQuery,
    sort: SortOrder,
    cursor: Option<&str>,
    query_hash: &str,
    feed_id_predicate: &FeedIdPredicate,
) -> Result<(String, Vec<Value>), AppError> {
    let (clauses, params) =
        build_non_tag_predicates(query, sort, cursor, query_hash, feed_id_predicate)?;
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("{}{}", q::WHERE_PREFIX, clauses.join(" AND "))
    };
    Ok((where_sql, params))
}

fn build_non_tag_predicates(
    query: &EntryQuery,
    sort: SortOrder,
    cursor: Option<&str>,
    query_hash: &str,
    feed_id_predicate: &FeedIdPredicate,
) -> Result<(Vec<String>, Vec<Value>), AppError> {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    if let Some(feed) = &query.feed {
        match feed {
            FeedFilter::Id(_) => match feed_id_predicate {
                FeedIdPredicate::Resolved(feed_pk) => {
                    clauses.push(q::ENTRY_FEED_PK_EQ.to_string());
                    params.push(Value::from(*feed_pk));
                }
                FeedIdPredicate::NotRequested => {
                    return Err(AppError::internal("feed id filter state is not resolved"));
                }
            },
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
    Ok((clauses, params))
}

fn build_entry_pk_filter_clause(entry_pks: &[i64]) -> (String, Vec<Value>) {
    if entry_pks.is_empty() {
        return ("0=1".to_string(), Vec::new());
    }
    let mut clauses = Vec::new();
    let mut params = Vec::with_capacity(entry_pks.len());
    for chunk in entry_pks.chunks(ENTRY_PK_CHUNK_SIZE) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        clauses.push(format!("e.id IN ({placeholders})"));
        for pk in chunk {
            params.push(Value::from(*pk));
        }
    }
    (clauses.join(" OR "), params)
}

fn fetch_entries(
    entry_repo: &EntryReadRepo<'_>,
    where_sql: &str,
    params: &[Value],
    sort: SortOrder,
    limit: usize,
    query_hash: &str,
) -> Result<EntryListPage, AppError> {
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
            error_details([
                ("kind", JsonValue::from("invalid_cursor")),
                ("field", JsonValue::from("cursor")),
                ("value", JsonValue::from(raw.to_string())),
                ("hint", JsonValue::from("base64url_decode_failed")),
            ]),
        )
    })?;
    let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|error| {
        AppError::invalid_query_with_details(
            format!("Invalid cursor: {error}"),
            error_details([
                ("kind", JsonValue::from("invalid_cursor")),
                ("field", JsonValue::from("cursor")),
                ("value", JsonValue::from(raw.to_string())),
                ("hint", JsonValue::from("cursor_json_decode_failed")),
            ]),
        )
    })?;
    if cursor.sort != sort.as_str() || cursor.query_hash != query_hash {
        return Err(AppError::invalid_query_with_details(
            "Cursor does not match the current query",
            error_details([
                ("kind", JsonValue::from("invalid_cursor")),
                ("field", JsonValue::from("cursor")),
                ("value", JsonValue::from(raw.to_string())),
                ("hint", JsonValue::from("cursor_mismatch")),
            ]),
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
fn build_tag_expr_clause(
    expr: &TagExpr,
    params: &mut Vec<Value>,
    resolved_tag_ids: &HashMap<String, i64>,
) -> String {
    match expr {
        TagExpr::Tag(tag) => match resolved_tag_ids.get(tag) {
            Some(id) => {
                params.push(Value::from(*id));
                q::EXISTS_TAG_ID_FOR_ENTRY.to_string()
            }
            None => "0=1".to_string(),
        },
        TagExpr::Not(inner) => {
            format!(
                "NOT ({})",
                build_tag_expr_clause(inner, params, resolved_tag_ids)
            )
        }
        TagExpr::And(items) => {
            let clauses = items
                .iter()
                .map(|item| {
                    format!(
                        "({})",
                        build_tag_expr_clause(item, params, resolved_tag_ids)
                    )
                })
                .collect::<Vec<_>>();
            clauses.join(" AND ")
        }
        TagExpr::Or(items) => {
            // When every child is a plain Tag, collapse into a single IN clause.
            if items.iter().all(|item| matches!(item, TagExpr::Tag(_))) {
                let ids: Vec<i64> = items
                    .iter()
                    .filter_map(|item| {
                        if let TagExpr::Tag(name) = item {
                            resolved_tag_ids.get(name).copied()
                        } else {
                            None
                        }
                    })
                    .collect();
                if ids.is_empty() {
                    return "0=1".to_string();
                }
                let placeholders = std::iter::repeat_n("?", ids.len())
                    .collect::<Vec<_>>()
                    .join(", ");
                for id in ids {
                    params.push(Value::from(id));
                }
                return q::exists_tag_ids_for_entry(&placeholders);
            }
            let clauses = items
                .iter()
                .map(|item| {
                    format!(
                        "({})",
                        build_tag_expr_clause(item, params, resolved_tag_ids)
                    )
                })
                .collect::<Vec<_>>();
            clauses.join(" OR ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TagEvalPath, route_tag_eval_path};
    use crate::query::TagExpr;

    #[test]
    fn route_complex_when_contains_not() {
        let expr = TagExpr::Not(Box::new(TagExpr::Tag("a".to_string())));
        assert_eq!(route_tag_eval_path(&expr), TagEvalPath::Complex);
    }

    #[test]
    fn route_simple_on_threshold_boundaries() {
        let expr = TagExpr::Or((0..6).map(|i| TagExpr::Tag(format!("t{i}"))).collect());
        assert_eq!(route_tag_eval_path(&expr), TagEvalPath::Simple);
    }

    #[test]
    fn route_complex_when_node_count_exceeds_threshold() {
        let expr = TagExpr::And((0..12).map(|i| TagExpr::Tag(format!("t{i}"))).collect());
        assert_eq!(route_tag_eval_path(&expr), TagEvalPath::Complex);
    }

    #[test]
    fn route_complex_when_depth_exceeds_threshold() {
        let expr = TagExpr::And(vec![
            TagExpr::Tag("a".to_string()),
            TagExpr::And(vec![
                TagExpr::Tag("b".to_string()),
                TagExpr::And(vec![
                    TagExpr::Tag("c".to_string()),
                    TagExpr::And(vec![
                        TagExpr::Tag("d".to_string()),
                        TagExpr::Tag("e".to_string()),
                    ]),
                ]),
            ]),
        ]);
        assert_eq!(route_tag_eval_path(&expr), TagEvalPath::Complex);
    }

    #[test]
    fn route_complex_when_or_fanout_exceeds_threshold() {
        let expr = TagExpr::Or((0..7).map(|i| TagExpr::Tag(format!("t{i}"))).collect());
        assert_eq!(route_tag_eval_path(&expr), TagEvalPath::Complex);
    }
}
