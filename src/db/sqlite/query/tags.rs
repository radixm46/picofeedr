//! Tag-related SQL queries.

/// Lists tags in ascending order.
pub(crate) const SELECT_TAGS_ORDERED: &str = "SELECT name FROM tags ORDER BY name ASC";

/// Inserts a tag if it does not exist.
pub(crate) const INSERT_TAG_ON_CONFLICT_NOTHING: &str =
    "INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING";

/// Builds SQL that resolves tag ids by names.
pub(crate) fn select_tag_ids_by_names(placeholders: &str) -> String {
    format!("SELECT id, name FROM tags WHERE name IN ({placeholders})")
}
