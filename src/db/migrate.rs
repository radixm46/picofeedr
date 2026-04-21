//! Database schema creation for SQLite.

use crate::db::sqlite::{initialize_meta_with_conn, schema};
use crate::error::{AppError, error_details};
use rusqlite::Connection;
use serde_json::Value;

/// Returns the current schema version.
pub fn current_schema_version() -> i64 {
    schema::CURRENT_SCHEMA_VERSION
}

/// Returns the on-disk SQLite schema version.
pub fn read_schema_version(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Creates the bootstrap schema for an empty database.
fn bootstrap_schema(conn: &Connection) -> Result<i64, AppError> {
    conn.execute_batch(schema::BOOTSTRAP_SCHEMA_SQL)?;
    conn.pragma_update(None, "user_version", schema::BOOTSTRAP_SCHEMA_VERSION)?;
    Ok(schema::BOOTSTRAP_SCHEMA_VERSION)
}

/// Applies forward-only migration steps until the target schema version is reached.
fn apply_migration_steps(
    conn: &Connection,
    mut version: i64,
    target_version: i64,
    migrations: &[schema::MigrationStep],
) -> Result<i64, AppError> {
    while version < target_version {
        let expected_to = version + 1;
        let step = migrations
            .iter()
            .find(|step| step.from == version && step.to == expected_to)
            .ok_or_else(|| {
                AppError::db_with_details(
                    format!(
                        "Missing migration path from schema version {version} to {target_version}"
                    ),
                    error_details([
                        ("db_schema_version", Value::from(version)),
                        ("expected_schema_version", Value::from(target_version)),
                        ("hint", Value::from("missing_migration_path")),
                    ]),
                )
            })?;
        conn.execute_batch(step.sql)?;
        conn.pragma_update(None, "user_version", step.to)?;
        version = step.to;
    }
    Ok(version)
}

/// Returns an error for unsupported on-disk schema versions.
fn unsupported_schema_version(version: i64) -> AppError {
    AppError::db_with_details(
        format!("Unsupported database schema version: {version}"),
        error_details([
            ("db_schema_version", Value::from(version)),
            (
                "expected_schema_version",
                Value::from(schema::CURRENT_SCHEMA_VERSION),
            ),
            ("hint", Value::from("upgrade_or_recreate_database")),
        ]),
    )
}

/// Applies schema bootstrap and initializes es_meta.
pub fn migrate(conn: &Connection) -> Result<(), AppError> {
    let mut version = read_schema_version(conn)?;
    if version == 0 {
        version = bootstrap_schema(conn)?;
    }

    if version > schema::CURRENT_SCHEMA_VERSION {
        return Err(unsupported_schema_version(version));
    }

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        version = apply_migration_steps(
            conn,
            version,
            schema::CURRENT_SCHEMA_VERSION,
            schema::MIGRATIONS,
        )?;
        if version != schema::CURRENT_SCHEMA_VERSION {
            return Err(unsupported_schema_version(version));
        }
        initialize_meta_with_conn(conn)?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_migration_steps, migrate, read_schema_version};
    use crate::db::sqlite::query::sync;
    use crate::db::sqlite::schema::MigrationStep;
    use crate::error::ErrorCode;
    use rusqlite::{Connection, params};
    use serde_json::Value;
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

    #[test]
    fn migrate_sets_sqlite_user_version() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");

        migrate(&conn).expect("migration should succeed");

        assert_eq!(
            read_schema_version(&conn).expect("schema version"),
            super::current_schema_version()
        );
    }

    #[test]
    fn migrate_initializes_meta_without_schema_version_field() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");

        migrate(&conn).expect("migration should succeed");

        let raw: String = conn
            .query_row(sync::SELECT_META_JSON, [], |row| row.get(0))
            .expect("load meta json");
        let meta: Value = serde_json::from_str(&raw).expect("valid meta json");
        assert_eq!(meta["app_id"], "picofeedr");
        assert!(meta.get("created_at").and_then(Value::as_i64).is_some());
        assert!(meta.get("schema_version").is_none());
    }

    #[test]
    fn migrate_is_idempotent_at_current_schema_version() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");

        migrate(&conn).expect("initial migration should succeed");
        migrate(&conn).expect("re-running migration should succeed");

        let row_count: i64 = conn
            .query_row(sync::COUNT_META_ROWS, [], |row| row.get(0))
            .expect("count meta rows");
        assert_eq!(row_count, 1);
        assert_eq!(
            read_schema_version(&conn).expect("schema version"),
            super::current_schema_version()
        );
    }

    #[test]
    fn migrate_rejects_unsupported_schema_version() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.pragma_update(None, "user_version", 99)
            .expect("set user_version");

        let error = migrate(&conn).expect_err("migration should fail");

        assert!(matches!(error.code(), ErrorCode::DbError));
        let details = error.details().expect("details");
        assert_eq!(details["db_schema_version"], 99);
        assert_eq!(
            details["expected_schema_version"],
            super::current_schema_version()
        );
        assert_eq!(details["hint"], "upgrade_or_recreate_database");
    }

    #[test]
    fn apply_migration_steps_applies_registered_steps_in_order() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute("CREATE TABLE migration_probe (value INTEGER NOT NULL)", [])
            .expect("create probe table");
        conn.execute("INSERT INTO migration_probe (value) VALUES (1)", [])
            .expect("insert probe row");
        conn.pragma_update(None, "user_version", 1)
            .expect("set initial version");

        let migrations = [
            MigrationStep {
                from: 1,
                to: 2,
                sql: "UPDATE migration_probe SET value = value + 1;",
            },
            MigrationStep {
                from: 2,
                to: 3,
                sql: "UPDATE migration_probe SET value = value * 10;",
            },
        ];

        let version =
            apply_migration_steps(&conn, 1, 3, &migrations).expect("apply migration steps");

        let value: i64 = conn
            .query_row("SELECT value FROM migration_probe", [], |row| row.get(0))
            .expect("load probe value");
        assert_eq!(version, 3);
        assert_eq!(value, 20);
        assert_eq!(read_schema_version(&conn).expect("schema version"), 3);
    }

    #[test]
    fn apply_migration_steps_rejects_missing_next_step() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        let migrations = [MigrationStep {
            from: 1,
            to: 3,
            sql: "SELECT 1;",
        }];

        let error =
            apply_migration_steps(&conn, 1, 3, &migrations).expect_err("missing step must fail");

        assert!(matches!(error.code(), ErrorCode::DbError));
        let details = error.details().expect("details");
        assert_eq!(details["db_schema_version"], 1);
        assert_eq!(details["expected_schema_version"], 3);
        assert_eq!(details["hint"], "missing_migration_path");
        assert_eq!(read_schema_version(&conn).expect("schema version"), 0);
    }

    #[test]
    fn migrate_enforces_non_empty_identifiers_and_known_storage_values() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&conn).expect("migration should succeed");

        assert!(
            conn.execute(
                "INSERT INTO feeds (feed_id, url, created_at) VALUES ('', 'https://example.com/feed', 1)",
                [],
            )
            .is_err(),
            "empty feed_id must be rejected"
        );
        assert!(
            conn.execute(
                "INSERT INTO feeds (feed_id, url, created_at) VALUES ('feed-1', '', 1)",
                [],
            )
            .is_err(),
            "empty feed url must be rejected"
        );

        conn.execute(
            "INSERT INTO feeds (feed_id, url, created_at) VALUES (?1, ?2, ?3)",
            params!["feed-1", "https://example.com/feed", 1],
        )
        .expect("insert feed");
        let feed_pk = conn.last_insert_rowid();

        assert!(
            conn.execute(
                "INSERT INTO entries (entry_id, feed_pk, first_seen_at) VALUES ('', ?1, 1)",
                params![feed_pk],
            )
            .is_err(),
            "empty entry_id must be rejected"
        );

        conn.execute(
            "INSERT INTO entries (entry_id, feed_pk, first_seen_at) VALUES (?1, ?2, ?3)",
            params!["entry-1", feed_pk, 1],
        )
        .expect("insert entry");
        let entry_pk = conn.last_insert_rowid();

        assert!(
            conn.execute(
                "INSERT INTO entry_contents (entry_pk, storage) VALUES (?1, ?2)",
                params![entry_pk, "invalid"],
            )
            .is_err(),
            "invalid entry_contents.storage must be rejected"
        );
        assert!(
            conn.execute("INSERT INTO tags (name) VALUES ('')", [])
                .is_err(),
            "empty tag name must be rejected"
        );
    }
}
