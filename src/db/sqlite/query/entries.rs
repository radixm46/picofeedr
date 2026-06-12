//! Entry-related SQL queries.

/// Inserts an entry if it does not already exist.
pub(crate) const INSERT_ENTRY: &str = "INSERT OR IGNORE INTO entries (\
        entry_id,\
        feed_pk,\
        link,\
        title,\
        author,\
        published_at,\
        updated_at,\
        first_seen_at,\
        meta_json\
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)";

/// Selects entry id from stable entry key.
pub(crate) const SELECT_ENTRY_PK_BY_ID: &str = "SELECT id FROM entries WHERE entry_id = ?1";

/// Upserts entry content.
pub(crate) const UPSERT_ENTRY_CONTENT: &str = "INSERT OR REPLACE INTO entry_contents (entry_pk, storage, ref, content_type, content)\
     VALUES (?1, ?2, ?3, ?4, ?5)";

/// Inserts a tag row if missing.
pub(crate) const INSERT_TAG_IGNORE: &str = "INSERT OR IGNORE INTO tags (name) VALUES (?1)";

/// Inserts entry-tag relation if missing.
pub(crate) const INSERT_ENTRY_TAG_IGNORE: &str =
    "INSERT OR IGNORE INTO entry_tags (entry_pk, tag_id) VALUES (?1, ?2)";

/// Creates temp tables used by bulk mark operations.
pub(crate) const CREATE_MARK_TEMP_TABLES: &str = r#"
CREATE TEMP TABLE IF NOT EXISTS temp_mark_entry_pks (
    entry_pk INTEGER PRIMARY KEY
);
CREATE TEMP TABLE IF NOT EXISTS temp_mark_add_tag_ids (
    tag_id INTEGER PRIMARY KEY
);
CREATE TEMP TABLE IF NOT EXISTS temp_mark_remove_tag_ids (
    tag_id INTEGER PRIMARY KEY
);
"#;

/// Clears staged rows from mark temp tables.
pub(crate) const CLEAR_MARK_TEMP_TABLES: &str = r#"
DELETE FROM temp_mark_entry_pks;
DELETE FROM temp_mark_add_tag_ids;
DELETE FROM temp_mark_remove_tag_ids;
"#;

/// Counts entries that would change during a bulk mark operation.
pub(crate) const COUNT_MARK_CHANGED_ENTRIES: &str = r#"
SELECT COUNT(DISTINCT entry_pk) FROM (
    SELECT tep.entry_pk AS entry_pk
    FROM temp_mark_entry_pks tep
    CROSS JOIN temp_mark_add_tag_ids tat
    LEFT JOIN entry_tags et
      ON et.entry_pk = tep.entry_pk AND et.tag_id = tat.tag_id
    WHERE et.entry_pk IS NULL
    UNION
    SELECT tep.entry_pk AS entry_pk
    FROM temp_mark_entry_pks tep
    CROSS JOIN temp_mark_remove_tag_ids trt
    JOIN entry_tags et
      ON et.entry_pk = tep.entry_pk AND et.tag_id = trt.tag_id
)
"#;

/// Inserts missing entry-tag relations for staged mark add operation.
pub(crate) const APPLY_MARK_ADDS: &str = r#"
INSERT OR IGNORE INTO entry_tags (entry_pk, tag_id)
SELECT tep.entry_pk, tat.tag_id
FROM temp_mark_entry_pks tep
CROSS JOIN temp_mark_add_tag_ids tat
"#;

/// Deletes staged entry-tag relations for mark remove operation.
pub(crate) const APPLY_MARK_REMOVES: &str = r#"
DELETE FROM entry_tags
WHERE EXISTS (
    SELECT 1
    FROM temp_mark_entry_pks tep
    JOIN temp_mark_remove_tag_ids trt
      ON tep.entry_pk = entry_tags.entry_pk
     AND trt.tag_id = entry_tags.tag_id
)
"#;

/// Counts all tags.
#[cfg(test)]
pub(crate) const COUNT_TAGS: &str = "SELECT COUNT(1) FROM tags";

/// Counts tag links for one entry.
#[cfg(test)]
pub(crate) const COUNT_ENTRY_TAGS_BY_ENTRY_ID: &str =
    "SELECT COUNT(1) FROM entry_tags WHERE entry_pk = ?1";

/// Reads one entry detail row joined with feed title.
pub(crate) const SELECT_ENTRY_DETAIL_BY_ID: &str = r#"
SELECT e.id, e.entry_id, f.feed_id, f.title, e.title, e.link, e.author, e.published_at, e.first_seen_at
FROM entries e
JOIN feeds f ON e.feed_pk = f.id
WHERE e.entry_id = ?1
"#;

/// Selects content row for one entry.
pub(crate) const SELECT_ENTRY_CONTENT_BY_ENTRY_ID: &str =
    "SELECT storage, ref, content_type, content FROM entry_contents WHERE entry_pk = ?1";

/// Selects enclosures for one entry.
pub(crate) const SELECT_ENTRY_ENCLOSURES_BY_ENTRY_ID: &str =
    "SELECT url, mime_type, length FROM entry_enclosures WHERE entry_pk = ?1 ORDER BY id";

/// Existence predicate for feed title filter.
pub(crate) const EXISTS_FEED_TITLE_FOR_ENTRY: &str =
    "EXISTS (SELECT 1 FROM feeds f WHERE f.id = e.feed_pk AND f.title = ?)";

/// Predicate for filtering by feed primary key.
pub(crate) const ENTRY_FEED_PK_EQ: &str = "e.feed_pk = ?";

/// Existence predicate for tag by resolved id.
pub(crate) const EXISTS_TAG_ID_FOR_ENTRY: &str =
    "EXISTS (SELECT 1 FROM entry_tags et WHERE et.entry_pk = e.id AND et.tag_id = ?)";

/// Builds existence predicate for OR-list of resolved tag ids.
pub(crate) fn exists_tag_ids_for_entry(placeholders: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM entry_tags et WHERE et.entry_pk = e.id AND et.tag_id IN ({placeholders}))"
    )
}

/// Prefix for WHERE clause construction.
pub(crate) const WHERE_PREFIX: &str = "WHERE ";

/// SQL expression for effective date sorting/filtering.
pub(crate) const EFFECTIVE_DATE_EXPR: &str =
    "COALESCE(e.published_at, e.updated_at, e.first_seen_at)";

/// ORDER BY clause for date descending sort.
pub(crate) const ORDER_BY_DATE_DESC: &str =
    "COALESCE(e.published_at, e.updated_at, e.first_seen_at) DESC, e.id DESC";

/// ORDER BY clause for date ascending sort.
pub(crate) const ORDER_BY_DATE_ASC: &str =
    "COALESCE(e.published_at, e.updated_at, e.first_seen_at) ASC, e.id ASC";

/// ORDER BY clause for first-seen descending sort.
pub(crate) const ORDER_BY_FIRST_SEEN_DESC: &str = "e.first_seen_at DESC, e.id DESC";

/// ORDER BY clause for first-seen ascending sort.
pub(crate) const ORDER_BY_FIRST_SEEN_ASC: &str = "e.first_seen_at ASC, e.id ASC";

/// Builds SQL that resolves tag ids by tag name.
pub(crate) fn select_tag_ids_by_names(placeholders: &str) -> String {
    format!("SELECT id, name FROM tags WHERE name IN ({placeholders})")
}

/// Builds SQL that counts entries with an optional WHERE clause.
pub(crate) fn count_entries(where_sql: &str) -> String {
    format!("SELECT COUNT(1) FROM entries e {where_sql}")
}

/// Builds SQL that fetches entry list rows with sort key.
pub(crate) fn fetch_entries(where_sql: &str, key_expr: &str, order_clause: &str) -> String {
    format!(
        "SELECT e.id, e.entry_id, f.feed_id, f.title, e.title, e.link, e.published_at, e.first_seen_at, {key_expr} AS sort_key \
         FROM entries e JOIN feeds f ON f.id = e.feed_pk {where_sql} ORDER BY {order_clause} LIMIT ?"
    )
}

/// Builds SQL that fetches internal entry ids by stable entry keys.
pub(crate) fn select_entry_pks_by_ids(placeholders: &str) -> String {
    format!("SELECT id, entry_id FROM entries WHERE entry_id IN ({placeholders})")
}

/// Builds SQL that inserts staged mark entry primary keys.
pub(crate) fn insert_temp_mark_entry_pks(placeholders: &str) -> String {
    format!("INSERT INTO temp_mark_entry_pks (entry_pk) VALUES {placeholders}")
}

/// Builds SQL that inserts staged mark add tag ids.
pub(crate) fn insert_temp_mark_add_tag_ids(placeholders: &str) -> String {
    format!("INSERT INTO temp_mark_add_tag_ids (tag_id) VALUES {placeholders}")
}

/// Builds SQL that inserts staged mark remove tag ids.
pub(crate) fn insert_temp_mark_remove_tag_ids(placeholders: &str) -> String {
    format!("INSERT INTO temp_mark_remove_tag_ids (tag_id) VALUES {placeholders}")
}

/// Builds SQL that fetches effective sort keys under where filters.
pub(crate) fn select_filtered_entry_sort_keys(where_sql: &str, key_expr: &str) -> String {
    format!("SELECT e.id, {key_expr} AS sort_key FROM entries e {where_sql}")
}

/// Builds SQL that fetches entry list rows by entry primary keys.
pub(crate) fn select_entry_rows_by_entry_pks(placeholders: &str) -> String {
    format!(
        "SELECT e.id, e.entry_id, f.feed_id, f.title, e.title, e.link, e.published_at, e.first_seen_at \
         FROM entries e JOIN feeds f ON f.id = e.feed_pk WHERE e.id IN ({placeholders})"
    )
}

/// Builds SQL that loads entry primary keys by tag ids.
pub(crate) fn select_entry_pks_by_tag_ids(tag_placeholders: &str) -> String {
    format!(
        "SELECT tag_id, entry_pk FROM entry_tags WHERE tag_id IN ({tag_placeholders}) ORDER BY tag_id, entry_pk"
    )
}

/// Builds SQL that loads tags for a set of entry ids.
pub(crate) fn load_tags_by_entry_ids(placeholders: &str) -> String {
    format!(
        "SELECT et.entry_pk, t.name FROM entry_tags et JOIN tags t ON et.tag_id = t.id \
         WHERE et.entry_pk IN ({placeholders}) ORDER BY t.name"
    )
}
