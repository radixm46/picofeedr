//! Feed-related SQL queries.

/// Selects all feeds in id order.
pub(crate) const SELECT_FEEDS: &str =
    "SELECT id, feed_key, url, title, author, site_url FROM feeds ORDER BY id";

/// Upserts feed record.
pub(crate) const UPSERT_FEED: &str = r#"
INSERT INTO feeds (feed_key, url, title, author, site_url, meta_json, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
ON CONFLICT(feed_key) DO UPDATE SET
   url = excluded.url,
   title = excluded.title,
   author = excluded.author,
   site_url = excluded.site_url,
   meta_json = excluded.meta_json,
   updated_at = excluded.updated_at
"#;

/// Finds feed primary keys by feed key for a dynamic IN list.
pub(crate) fn select_feed_pks_by_keys(placeholders: &str) -> String {
    format!("SELECT feed_key, id FROM feeds WHERE feed_key IN ({placeholders})")
}

/// Selects one feed id by feed key.
#[cfg(test)]
pub(crate) const SELECT_FEED_ID_BY_KEY: &str = "SELECT id FROM feeds WHERE feed_key = ?1";
