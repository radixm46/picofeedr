use super::{EntryListResponse, EntrySummary, FeedSummary};
mod cursor;
mod setops;

use crate::cli::SortOrder;
use crate::db::sqlite::SqliteStore;
use crate::db::sqlite::query::{entries as q, sql_placeholders};
use crate::db::sqlite::repo::{EntryListRow, EntryReadRepo};
use crate::error::{AppError, error_details};
use crate::query::{EntryQuery, FeedFilter, TagExpr};
use cursor::{compute_query_hash, decode_cursor, encode_cursor_with_query};
use rusqlite::types::Value;
use serde_json::Value as JsonValue;
use setops::{UniverseView, intersect_sorted_into, merge_union_sorted_into};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
    if let Some(tag_expr) = &query.tag_expr
        && matches!(route_tag_eval_path(tag_expr), TagEvalPath::Complex)
    {
        let (universe_where_sql, universe_params) = build_where_clause(
            query,
            sort,
            None,
            &query_hash,
            &feed_id_predicate,
            None,
            &[],
        )?;
        let mut universe_sort_pairs = entry_repo.list_filtered_entry_sort_keys(
            &universe_where_sql,
            &universe_params,
            sort_key_expr(sort),
        )?;
        universe_sort_pairs.sort_unstable_by_key(|(entry_pk, _)| *entry_pk);
        universe_sort_pairs.dedup_by_key(|(entry_pk, _)| *entry_pk);
        let matched_entry_pks = resolve_complex_tag_entry_pks(
            &entry_repo,
            &universe_sort_pairs,
            &resolved_tag_ids,
            tag_expr,
        )?;
        let total_count = matched_entry_pks.len() as i64;
        let (items, feeds, next_page_token) = fetch_entries_complex(
            &entry_repo,
            &universe_sort_pairs,
            &matched_entry_pks,
            sort,
            limit,
            cursor,
            &query_hash,
        )?;
        return Ok(EntryListResponse {
            total_count,
            items,
            feeds,
            next_page_token,
            revision: system_meta.revision,
            last_write_at: system_meta.updated_at,
            last_sync_at: system_meta.sync_at,
        });
    }

    let (tag_clause, tag_params) = match &query.tag_expr {
        Some(tag_expr) => {
            let mut params = Vec::new();
            let clause = build_tag_expr_clause(tag_expr, &mut params, &resolved_tag_ids);
            (Some(clause), params)
        }
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
        last_sync_at: system_meta.sync_at,
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
    universe_sort_pairs: &[(i64, i64)],
    resolved_tag_ids: &HashMap<String, i64>,
    tag_expr: &TagExpr,
) -> Result<Vec<i64>, AppError> {
    // NOTE: This set evaluation is recomputed for every list request (including cursor pages).
    // Keeping it stateless preserves correctness, and caching can be considered in a follow-up.
    if universe_sort_pairs.is_empty() {
        return Ok(Vec::new());
    }
    let tag_ids = resolved_tag_ids.values().copied().collect::<Vec<_>>();
    let tag_entry_pks = entry_repo.find_entry_pks_by_tag_ids(&tag_ids)?;
    let matched = evaluate_tag_expr_set(
        tag_expr,
        UniverseView(universe_sort_pairs),
        resolved_tag_ids,
        &tag_entry_pks,
    );
    Ok(matched)
}

fn evaluate_tag_expr_set(
    expr: &TagExpr,
    universe: UniverseView<'_>,
    resolved_tag_ids: &HashMap<String, i64>,
    tag_entry_pks: &HashMap<i64, Vec<i64>>,
) -> Vec<i64> {
    match expr {
        TagExpr::Tag(tag) => {
            let Some(tag_id) = resolved_tag_ids.get(tag) else {
                return Vec::new();
            };
            let Some(entry_pks) = tag_entry_pks.get(tag_id) else {
                return Vec::new();
            };
            universe.intersect_sorted(entry_pks)
        }
        TagExpr::Not(inner) => {
            let inner_set = evaluate_tag_expr_set(inner, universe, resolved_tag_ids, tag_entry_pks);
            universe.difference_sorted(&inner_set)
        }
        TagExpr::And(items) => {
            let mut iter = items.iter();
            let Some(first) = iter.next() else {
                return Vec::new();
            };
            let mut acc = evaluate_tag_expr_set(first, universe, resolved_tag_ids, tag_entry_pks);
            let mut scratch = Vec::new();
            for item in iter {
                let next = evaluate_tag_expr_set(item, universe, resolved_tag_ids, tag_entry_pks);
                if acc.is_empty() || next.is_empty() {
                    return Vec::new();
                }
                intersect_sorted_into(&acc, &next, &mut scratch);
                std::mem::swap(&mut acc, &mut scratch);
                if acc.is_empty() {
                    break;
                }
            }
            acc
        }
        TagExpr::Or(items) => {
            let mut iter = items.iter();
            let Some(first) = iter.next() else {
                return Vec::new();
            };
            let mut acc = evaluate_tag_expr_set(first, universe, resolved_tag_ids, tag_entry_pks);
            let mut scratch = Vec::new();
            if acc.len() == universe.len() {
                return acc;
            }
            for item in iter {
                let next = evaluate_tag_expr_set(item, universe, resolved_tag_ids, tag_entry_pks);
                if next.is_empty() {
                    continue;
                }
                merge_union_sorted_into(&acc, &next, &mut scratch);
                std::mem::swap(&mut acc, &mut scratch);
                if acc.len() == universe.len() {
                    break;
                }
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
    let ids: Vec<i64> = rows.iter().map(|row| row.entry_pk).collect();
    let (items, feeds) = finalize_rows(entry_repo, rows)?;
    let next_page_token = if has_next {
        match (ids.last(), sort_keys.last()) {
            (Some(id), Some(key)) => Some(encode_cursor_with_query(*key, *id, sort, query_hash)?),
            _ => None,
        }
    } else {
        None
    };
    Ok((items, feeds, next_page_token))
}

fn fetch_entries_complex(
    entry_repo: &EntryReadRepo<'_>,
    universe_sort_pairs: &[(i64, i64)],
    matched_pks: &[i64],
    sort: SortOrder,
    limit: usize,
    cursor: Option<&str>,
    query_hash: &str,
) -> Result<EntryListPage, AppError> {
    let mut sort_pairs = universe_sort_pairs
        .iter()
        .copied()
        .filter(|(entry_id, _)| matched_pks.binary_search(entry_id).is_ok())
        .collect::<Vec<_>>();
    sort_pairs.sort_unstable_by(|(id_a, key_a), (id_b, key_b)| match sort {
        SortOrder::DateDesc | SortOrder::FirstSeenDesc => {
            key_b.cmp(key_a).then_with(|| id_b.cmp(id_a))
        }
        SortOrder::DateAsc | SortOrder::FirstSeenAsc => {
            key_a.cmp(key_b).then_with(|| id_a.cmp(id_b))
        }
    });

    if let Some(raw_cursor) = cursor {
        let decoded = decode_cursor(raw_cursor, sort, query_hash)?;
        sort_pairs.retain(|(entry_id, sort_key)| match sort {
            SortOrder::DateDesc | SortOrder::FirstSeenDesc => {
                (*sort_key, *entry_id) < (decoded.k, decoded.id)
            }
            SortOrder::DateAsc | SortOrder::FirstSeenAsc => {
                (*sort_key, *entry_id) > (decoded.k, decoded.id)
            }
        });
    }

    let has_next = sort_pairs.len() > limit;
    if has_next {
        sort_pairs.truncate(limit);
    }
    let page_pairs = sort_pairs;

    let page_ids = page_pairs
        .iter()
        .map(|(entry_id, _)| *entry_id)
        .collect::<Vec<_>>();
    let rows = entry_repo.load_entry_rows_by_entry_pks(&page_ids)?;
    let mut rows_by_id = rows
        .into_iter()
        .map(|row| (row.entry_pk, row))
        .collect::<HashMap<_, _>>();
    let mut ordered_rows = Vec::with_capacity(page_ids.len());
    for entry_id in &page_ids {
        if let Some(row) = rows_by_id.remove(entry_id) {
            ordered_rows.push(row);
        }
    }

    let (items, feeds) = finalize_rows(entry_repo, ordered_rows)?;
    let next_page_token = if has_next {
        page_pairs
            .last()
            .map(|(id, key)| encode_cursor_with_query(*key, *id, sort, query_hash))
            .transpose()?
    } else {
        None
    };
    Ok((items, feeds, next_page_token))
}

fn finalize_rows(
    entry_repo: &EntryReadRepo<'_>,
    mut rows: Vec<EntryListRow>,
) -> Result<(Vec<EntrySummary>, Vec<FeedSummary>), AppError> {
    let ids: Vec<i64> = rows.iter().map(|row| row.entry_pk).collect();
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
    let items = rows.into_iter().map(|row| row.summary).collect::<Vec<_>>();
    Ok((items, feeds))
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
                let placeholders = sql_placeholders(ids.len());
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
    use super::setops::UniverseView;
    use super::{TagEvalPath, evaluate_tag_expr_set, route_tag_eval_path};
    use crate::query::TagExpr;
    use std::collections::HashMap;

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

    #[test]
    fn evaluate_tag_expr_set_handles_nested_or_and_not() {
        let expr = TagExpr::Or(vec![
            TagExpr::And(vec![
                TagExpr::Tag("a".to_string()),
                TagExpr::Tag("b".to_string()),
            ]),
            TagExpr::Not(Box::new(TagExpr::Tag("c".to_string()))),
        ]);
        let universe = vec![(1, 100), (2, 90), (3, 80), (4, 70), (5, 60)];
        let resolved_tag_ids = HashMap::from([
            ("a".to_string(), 10),
            ("b".to_string(), 20),
            ("c".to_string(), 30),
        ]);
        let tag_entry_pks =
            HashMap::from([(10, vec![1, 3, 5]), (20, vec![3, 4, 5]), (30, vec![2, 5])]);

        let matched = evaluate_tag_expr_set(
            &expr,
            UniverseView(&universe),
            &resolved_tag_ids,
            &tag_entry_pks,
        );

        assert_eq!(matched, vec![1, 3, 4, 5]);
    }

    #[test]
    fn evaluate_tag_expr_set_accepts_pair_universe_without_materializing_ids() {
        let expr = TagExpr::Or(vec![
            TagExpr::And(vec![
                TagExpr::Tag("a".to_string()),
                TagExpr::Tag("b".to_string()),
            ]),
            TagExpr::Not(Box::new(TagExpr::Tag("c".to_string()))),
        ]);
        let universe_pairs = vec![(1, 100), (2, 90), (3, 80), (4, 70), (5, 60)];
        let resolved_tag_ids = HashMap::from([
            ("a".to_string(), 10),
            ("b".to_string(), 20),
            ("c".to_string(), 30),
        ]);
        let tag_entry_pks =
            HashMap::from([(10, vec![1, 3, 5]), (20, vec![3, 4, 5]), (30, vec![2, 5])]);

        let matched = evaluate_tag_expr_set(
            &expr,
            UniverseView(&universe_pairs),
            &resolved_tag_ids,
            &tag_entry_pks,
        );

        assert_eq!(matched, vec![1, 3, 4, 5]);
    }
}
