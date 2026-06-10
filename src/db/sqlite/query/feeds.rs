//! Feed-related SQL queries.

/// Selects all feeds in id order.
pub(crate) const SELECT_FEEDS: &str =
    "SELECT id, feed_id, url, title, author, site_url FROM feeds ORDER BY id";

/// Upserts feed record.
pub(crate) const UPSERT_FEED: &str = r#"
INSERT INTO feeds (feed_id, url, title, author, site_url, meta_json, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
ON CONFLICT(feed_id) DO UPDATE SET
   url = excluded.url,
   title = excluded.title,
   author = excluded.author,
   site_url = excluded.site_url,
   meta_json = excluded.meta_json,
   updated_at = excluded.updated_at
"#;

/// Upserts feed config fields without clobbering observed metadata.
///
/// feed_id is derived from the url, so a conflicting row already holds
/// the same url and only config-owned title needs refreshing.
pub(crate) const UPSERT_FEED_FROM_CONFIG: &str = r#"
INSERT INTO feeds (feed_id, url, title, author, site_url, meta_json, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
ON CONFLICT(feed_id) DO UPDATE SET
   title = COALESCE(excluded.title, title),
   updated_at = excluded.updated_at
"#;

/// Refreshes observed feed metadata on a known feed row.
pub(crate) const UPDATE_FEED_METADATA: &str = r#"
UPDATE feeds
SET title = COALESCE(title, ?1),
    author = COALESCE(?2, author),
    site_url = COALESCE(?3, site_url),
    updated_at = ?4
WHERE id = ?5
"#;

/// Finds feed primary keys by feed key for a dynamic IN list.
pub(crate) fn select_feed_pks_by_ids(placeholders: &str) -> String {
    format!("SELECT feed_id, id FROM feeds WHERE feed_id IN ({placeholders})")
}

/// Selects one feed id by feed key.
#[cfg(test)]
pub(crate) const SELECT_FEED_PK_BY_ID: &str = "SELECT id FROM feeds WHERE feed_id = ?1";
