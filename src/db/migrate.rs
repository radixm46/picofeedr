//! Database schema creation for SQLite.

use crate::db::sqlite::query::sync;
use crate::db::sqlite::schema;
use crate::error::AppError;
use crate::time::current_epoch;
use rusqlite::Connection;
use serde_json::json;

/// Returns the current schema version.
pub fn current_schema_version() -> i64 {
    schema::CURRENT_SCHEMA_VERSION
}

/// Applies schema migrations and initializes es_meta.
pub fn migrate(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(schema::V1_SCHEMA_SQL)?;
    let exists: i64 = conn.query_row(sync::COUNT_META_ROWS, [], |row| row.get(0))?;
    if exists == 0 {
        let meta_json = json!({
            "schema_version": 1,
            "created_at": current_epoch(),
            "app_id": "picofeedr"
        })
        .to_string();
        conn.execute(sync::INSERT_META_ROW, [&meta_json])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::migrate;
    use rusqlite::Connection;
    use std::collections::HashSet;

    /// Returns index names defined on one table.
    fn table_index_names(conn: &Connection, table: &str) -> rusqlite::Result<HashSet<String>> {
        let mut stmt = conn.prepare(&format!("PRAGMA index_list('{table}')"))?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        Ok(names)
    }

    /// Returns column names for one table.
    fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<HashSet<String>> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info('{table}')"))?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        Ok(names)
    }

    /// Migration should create expression indexes used by effective-date sorting.
    #[test]
    fn migrate_creates_effective_date_expression_indexes() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migration should succeed");

        let names = table_index_names(&conn, "entries").expect("index list should be queryable");
        assert!(
            names.contains("idx_entries_effective_date"),
            "idx_entries_effective_date is missing"
        );
        assert!(
            names.contains("idx_entries_feed_effective_date"),
            "idx_entries_feed_effective_date is missing"
        );
    }

    /// Migration should expose DB column names aligned with public id vocabulary.
    #[test]
    fn migrate_uses_feed_id_and_entry_id_columns() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migration should succeed");

        let feed_columns = table_columns(&conn, "feeds").expect("feeds table columns");
        assert!(feed_columns.contains("feed_id"));
        assert!(!feed_columns.contains("feed_key"));

        let entry_columns = table_columns(&conn, "entries").expect("entries table columns");
        assert!(entry_columns.contains("entry_id"));
        assert!(entry_columns.contains("feed_pk"));
        assert!(!entry_columns.contains("source_id"));
        assert!(!entry_columns.contains("entry_key"));
        assert!(!entry_columns.contains("feed_id"));
    }

    /// Migration should not create deprecated source-id index on entries.
    #[test]
    fn migrate_does_not_create_feed_source_index() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migration should succeed");

        let names = table_index_names(&conn, "entries").expect("index list should be queryable");
        assert!(!names.contains("idx_entries_feed_source"));
    }

    /// Migration should not create redundant unused indexes in current schema.
    #[test]
    fn migrate_does_not_create_redundant_unused_indexes() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migration should succeed");

        let entry_names =
            table_index_names(&conn, "entries").expect("entries index list should be queryable");
        assert!(!entry_names.contains("idx_entries_feed_pk"));
        assert!(!entry_names.contains("idx_entries_link"));

        let feed_names =
            table_index_names(&conn, "feeds").expect("feeds index list should be queryable");
        assert!(!feed_names.contains("idx_feeds_url"));
        assert!(!feed_names.contains("idx_feeds_feed_id"));

        assert!(!entry_names.contains("idx_entries_entry_id"));

        let tag_names =
            table_index_names(&conn, "tags").expect("tags index list should be queryable");
        assert!(!tag_names.contains("idx_tags_name"));

        let enclosure_names = table_index_names(&conn, "entry_enclosures")
            .expect("entry_enclosures index list should be queryable");
        assert!(!enclosure_names.contains("idx_entry_enclosures_entry_pk"));

        let entry_tag_names = table_index_names(&conn, "entry_tags")
            .expect("entry_tags index list should be queryable");
        assert!(!entry_tag_names.contains("idx_entry_tags_entry_pk"));
    }
}
