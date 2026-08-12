use super::{EntryListResponse, EntrySummary, FeedSummary};
mod cursor;
mod setops;

use crate::cli::SortOrder;
use crate::db::sqlite::SqliteStore;
use crate::db::sqlite::repo::{EntryListFilter, EntryListRow, EntryListSort, EntryReadRepo};
use crate::error::{AppError, error_details};
use crate::query::{EntryQuery, FeedFilter, TagExpr, TermExpr};
use cursor::{Cursor, compute_query_hash, decode_cursor, encode_cursor_with_query};
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
    let system_meta = store.read_system_meta()?;
    let query_hash = compute_query_hash(query);
    let feed_id_predicate = resolve_feed_id_predicate(store, query)?;
    let resolved_tag_ids = resolve_tag_id_map(store, query)?;
    let repo_sort = entry_list_sort(sort);
    if let Some(tag_expr) = &query.tag_expr
        && matches!(route_tag_eval_path(tag_expr), TagEvalPath::Complex)
    {
        let universe_filters = build_list_filters(query, &feed_id_predicate, None, None)?;
        let mut universe_sort_pairs =
            entry_repo.list_filtered_entry_sort_keys(&universe_filters, repo_sort)?;
        universe_sort_pairs.sort_unstable_by_key(|(entry_pk, _)| *entry_pk);
        universe_sort_pairs.dedup_by_key(|(entry_pk, _)| *entry_pk);
        let matched_entry_pks = resolve_complex_tag_entry_pks(
            &entry_repo,
            &universe_sort_pairs,
            &resolved_tag_ids,
            tag_expr,
        )?;
        let total_count = matched_entry_pks.len() as i64;
        let decoded_cursor = cursor
            .map(|raw| decode_cursor(raw, sort, &query_hash))
            .transpose()?;
        let (items, feeds, next_page_token) = fetch_entries_complex(
            &entry_repo,
            &universe_sort_pairs,
            &matched_entry_pks,
            sort,
            limit,
            decoded_cursor.as_ref(),
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

    let tag_filter = query
        .tag_expr
        .as_ref()
        .map(|expr| build_tag_filter(expr, &resolved_tag_ids));
    let count_filters = build_list_filters(query, &feed_id_predicate, tag_filter.as_ref(), None)?;
    let total_count = entry_repo.count_entries(&count_filters, repo_sort)?;
    let decoded_cursor = cursor
        .map(|raw| decode_cursor(raw, sort, &query_hash))
        .transpose()?;
    let page_filters = build_list_filters(
        query,
        &feed_id_predicate,
        tag_filter.as_ref(),
        decoded_cursor.as_ref(),
    )?;
    let (items, feeds, next_page_token) =
        fetch_entries(&entry_repo, &page_filters, sort, limit, &query_hash)?;
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
    let resolved = store.find_feed_pks_by_ids(std::slice::from_ref(feed_id))?;
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

fn build_list_filters(
    query: &EntryQuery,
    feed_id_predicate: &FeedIdPredicate,
    tag_filter: Option<&EntryListFilter>,
    cursor: Option<&Cursor>,
) -> Result<Vec<EntryListFilter>, AppError> {
    let mut filters = Vec::new();
    if let Some(feed) = &query.feed {
        match feed {
            FeedFilter::Id(_) => match feed_id_predicate {
                FeedIdPredicate::Resolved(feed_pk) => {
                    filters.push(EntryListFilter::FeedPk(*feed_pk));
                }
                FeedIdPredicate::NotRequested => {
                    return Err(AppError::internal("feed id filter state is not resolved"));
                }
            },
            FeedFilter::Title(title) => {
                filters.push(EntryListFilter::FeedTitle(title.clone()));
            }
        }
    }
    for title in &query.title_terms {
        filters.push(EntryListFilter::TitleContains(title.clone()));
    }
    for title in &query.negated_title_terms {
        filters.push(EntryListFilter::Not(Box::new(
            EntryListFilter::TitleContains(title.clone()),
        )));
    }
    for expr in &query.term_groups {
        filters.push(build_term_filter(expr));
    }
    for expr in &query.negated_term_groups {
        filters.push(EntryListFilter::Not(Box::new(build_term_filter(expr))));
    }
    if let Some(after) = query.after {
        filters.push(EntryListFilter::EffectiveDateAtLeast(after));
    }
    if let Some(before) = query.before {
        filters.push(EntryListFilter::EffectiveDateBefore(before));
    }
    if let Some(cursor) = cursor {
        filters.push(EntryListFilter::Cursor {
            key: cursor.k,
            entry_pk: cursor.id,
        });
    }
    if let Some(tag_filter) = tag_filter {
        filters.push(tag_filter.clone());
    }
    Ok(filters)
}

fn build_term_filter(expr: &TermExpr) -> EntryListFilter {
    match expr {
        TermExpr::Term(term) => EntryListFilter::TitleContains(term.clone()),
        TermExpr::Not(inner) => EntryListFilter::Not(Box::new(build_term_filter(inner))),
        TermExpr::And(items) => EntryListFilter::And(items.iter().map(build_term_filter).collect()),
        TermExpr::Or(items) => EntryListFilter::Or(items.iter().map(build_term_filter).collect()),
    }
}

fn build_tag_filter(expr: &TagExpr, resolved_tag_ids: &HashMap<String, i64>) -> EntryListFilter {
    match expr {
        TagExpr::Tag(tag) => resolved_tag_ids
            .get(tag)
            .copied()
            .map(|tag_id| EntryListFilter::TagIds(vec![tag_id]))
            .unwrap_or_else(|| EntryListFilter::TagIds(Vec::new())),
        TagExpr::Not(inner) => {
            EntryListFilter::Not(Box::new(build_tag_filter(inner, resolved_tag_ids)))
        }
        TagExpr::And(items) => EntryListFilter::And(
            items
                .iter()
                .map(|item| build_tag_filter(item, resolved_tag_ids))
                .collect(),
        ),
        TagExpr::Or(items) => {
            if items.iter().all(|item| matches!(item, TagExpr::Tag(_))) {
                let ids = items
                    .iter()
                    .filter_map(|item| {
                        if let TagExpr::Tag(name) = item {
                            resolved_tag_ids.get(name).copied()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                EntryListFilter::TagIds(ids)
            } else {
                EntryListFilter::Or(
                    items
                        .iter()
                        .map(|item| build_tag_filter(item, resolved_tag_ids))
                        .collect(),
                )
            }
        }
    }
}

fn fetch_entries(
    entry_repo: &EntryReadRepo<'_>,
    filters: &[EntryListFilter],
    sort: SortOrder,
    limit: usize,
    query_hash: &str,
) -> Result<EntryListPage, AppError> {
    let (mut rows, mut sort_keys) =
        entry_repo.fetch_entries(filters, entry_list_sort(sort), limit)?;
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
    cursor: Option<&Cursor>,
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

    if let Some(decoded) = cursor {
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
    rows: Vec<EntryListRow>,
) -> Result<(Vec<EntrySummary>, Vec<FeedSummary>), AppError> {
    let ids: Vec<i64> = rows.iter().map(|row| row.entry_pk).collect();
    let tags = entry_repo.load_tags(&ids)?;
    let feeds = rows
        .iter()
        .fold(BTreeMap::<String, Option<String>>::new(), |mut map, row| {
            map.entry(row.feed_id.clone())
                .or_insert_with(|| row.feed_title.clone());
            map
        })
        .into_iter()
        .map(|(feed_id, title)| FeedSummary { feed_id, title })
        .collect::<Vec<_>>();
    let items = rows
        .into_iter()
        .map(|row| EntrySummary {
            entry_id: row.entry_id,
            feed_id: row.feed_id,
            title: row.title,
            link: row.link,
            published_at: row.published_at,
            first_seen_at: row.first_seen_at,
            tags: tags.get(&row.entry_pk).cloned().unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    Ok((items, feeds))
}

fn entry_list_sort(sort: SortOrder) -> EntryListSort {
    match sort {
        SortOrder::DateDesc => EntryListSort::DateDesc,
        SortOrder::DateAsc => EntryListSort::DateAsc,
        SortOrder::FirstSeenDesc => EntryListSort::FirstSeenDesc,
        SortOrder::FirstSeenAsc => EntryListSort::FirstSeenAsc,
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
