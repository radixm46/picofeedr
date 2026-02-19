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
            "schema_version": schema::CURRENT_SCHEMA_VERSION,
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

    /// Returns CREATE TABLE SQL for one table.
    fn table_create_sql(conn: &Connection, table: &str) -> rusqlite::Result<String> {
        conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
    }

    /// Asserts that all expected index names are absent from the table.
    fn assert_indexes_absent(conn: &Connection, table: &str, names: &[&str]) {
        let actual = table_index_names(conn, table).expect("index list should be queryable");
        for name in names {
            assert!(
                !actual.contains(*name),
                "unexpected index {name} exists on {table}"
            );
        }
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

    /// Migration should not create removed/redundant indexes in current schema.
    #[test]
    fn migrate_does_not_create_removed_indexes() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migration should succeed");

        assert_indexes_absent(
            &conn,
            "entries",
            &[
                "idx_entries_feed_source",
                "idx_entries_feed_pk",
                "idx_entries_link",
                "idx_entries_entry_id",
            ],
        );
        assert_indexes_absent(&conn, "feeds", &["idx_feeds_url", "idx_feeds_feed_id"]);
        assert_indexes_absent(&conn, "tags", &["idx_tags_name"]);
        assert_indexes_absent(
            &conn,
            "entry_enclosures",
            &["idx_entry_enclosures_entry_pk"],
        );
        assert_indexes_absent(&conn, "entry_tags", &["idx_entry_tags_entry_pk"]);
        assert_indexes_absent(&conn, "entry_tags", &["idx_entry_tags_tag"]);
    }

    /// Migration should create entry_tags as WITHOUT ROWID.
    #[test]
    fn migrate_creates_entry_tags_without_rowid() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migration should succeed");

        let sql = table_create_sql(&conn, "entry_tags").expect("entry_tags create sql");
        assert!(
            sql.to_ascii_uppercase().contains("WITHOUT ROWID"),
            "entry_tags is expected to use WITHOUT ROWID: {sql}"
        );
    }
}
