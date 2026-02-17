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
    use crate::db::sqlite::query::sync;
    use rusqlite::Connection;
    use std::collections::HashSet;

    /// Returns index names defined on the entries table.
    fn entries_index_names(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
        let mut stmt = conn.prepare(sync::PRAGMA_INDEX_LIST_ENTRIES)?;
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

        let names = entries_index_names(&conn).expect("index list should be queryable");
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
        assert!(!entry_columns.contains("entry_key"));
        assert!(!entry_columns.contains("feed_id"));
    }
}
