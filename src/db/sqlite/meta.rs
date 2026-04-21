//! Metadata DAO helpers backed by `es_meta.meta_json`.
//!
//! This module intentionally stays at single-statement query execution level.
//! Multi-step workflows must live in repository modules.

use crate::db::sqlite::query::sync;
use crate::error::AppError;
use crate::time::current_epoch;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// Database-wide status metadata used by `status` and list snapshots.
#[derive(Debug, Clone)]
pub struct SystemMeta {
    /// Monotonic revision incremented after successful write commands.
    pub revision: i64,
    /// Epoch seconds of the latest successful write command.
    pub updated_at: Option<i64>,
    /// Epoch seconds of the latest successful sync command.
    pub sync_at: Option<i64>,
    /// Status of the latest successful sync command.
    pub sync_status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredSystemMeta {
    #[serde(default)]
    revision: i64,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<i64>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_at: Option<i64>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_status: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl From<StoredSystemMeta> for SystemMeta {
    fn from(value: StoredSystemMeta) -> Self {
        Self {
            revision: value.revision,
            updated_at: value.updated_at,
            sync_at: value.sync_at,
            sync_status: value.sync_status,
        }
    }
}

/// Ensures `es_meta` contains the initial metadata row.
pub(crate) fn initialize_meta_with_conn(conn: &Connection) -> Result<(), AppError> {
    let exists: i64 = conn.query_row(sync::COUNT_META_ROWS, [], |row| row.get(0))?;
    if exists == 0 {
        let meta_json = json!({
            "created_at": current_epoch(),
            "app_id": "picofeedr"
        })
        .to_string();
        conn.execute(sync::INSERT_META_ROW, params![meta_json])?;
    }
    Ok(())
}

/// Loads system metadata from `es_meta.meta_json`.
pub(crate) fn read_meta_with_conn(conn: &Connection) -> Result<SystemMeta, AppError> {
    let meta_json = load_meta_json_text_with_conn(conn)?;
    parse_meta(&meta_json)
}

/// Increments `revision` and updates `updated_at`.
pub(crate) fn bump_revision_with_conn(conn: &Connection, now: i64) -> Result<SystemMeta, AppError> {
    let meta_json = load_meta_json_text_with_conn(conn)?;
    let mut meta = parse_meta_object(&meta_json)?;
    meta.revision = meta.revision.saturating_add(1);
    meta.updated_at = Some(now);
    write_meta_json_object_with_conn(conn, &meta)
}

/// Updates `sync_at` and `sync_status`.
pub(crate) fn update_sync_with_conn(
    conn: &Connection,
    now: i64,
    status: &str,
) -> Result<SystemMeta, AppError> {
    let meta_json = load_meta_json_text_with_conn(conn)?;
    let mut meta = parse_meta_object(&meta_json)?;
    meta.sync_at = Some(now);
    meta.sync_status = Some(status.to_string());
    write_meta_json_object_with_conn(conn, &meta)
}

/// Reads raw `meta_json` text from `es_meta`.
fn load_meta_json_text_with_conn(conn: &Connection) -> Result<String, AppError> {
    conn.query_row(sync::SELECT_META_JSON, [], |row| row.get(0))
        .map_err(AppError::from)
}

/// Parses `meta_json` text into persisted metadata.
fn parse_meta_object(meta_json: &str) -> Result<StoredSystemMeta, AppError> {
    let value = serde_json::from_str::<Value>(meta_json)?;
    match value {
        Value::Object(_) => serde_json::from_value(value).map_err(AppError::from),
        _ => Ok(StoredSystemMeta::default()),
    }
}

/// Writes persisted metadata back into `es_meta.meta_json`.
fn write_meta_json_object_with_conn(
    conn: &Connection,
    meta: &StoredSystemMeta,
) -> Result<SystemMeta, AppError> {
    let meta_json = serde_json::to_string(meta)?;
    conn.execute(sync::UPDATE_META_JSON, params![meta_json])?;
    Ok(meta.clone().into())
}

/// Parses status fields from `meta_json` text.
fn parse_meta(meta_json: &str) -> Result<SystemMeta, AppError> {
    Ok(parse_meta_object(meta_json)?.into())
}

#[cfg(test)]
mod tests {
    use super::{
        bump_revision_with_conn, initialize_meta_with_conn, read_meta_with_conn,
        update_sync_with_conn,
    };
    use crate::db::sqlite::query::sync;
    use rusqlite::{Connection, params};
    use serde_json::{Value, json};

    fn setup_conn(meta_json: &str) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute(
            "CREATE TABLE es_meta (id INTEGER PRIMARY KEY, meta_json TEXT NOT NULL)",
            [],
        )
        .expect("create meta table");
        conn.execute(sync::INSERT_META_ROW, params![meta_json])
            .expect("insert meta row");
        conn
    }

    fn load_raw_meta_json(conn: &Connection) -> Value {
        let raw: String = conn
            .query_row(sync::SELECT_META_JSON, [], |row| row.get(0))
            .expect("load meta json");
        serde_json::from_str(&raw).expect("valid json")
    }

    #[test]
    fn read_meta_with_conn_treats_non_object_payload_as_default() {
        let conn = setup_conn("null");

        let meta = read_meta_with_conn(&conn).expect("meta");

        assert_eq!(meta.revision, 0);
        assert_eq!(meta.updated_at, None);
        assert_eq!(meta.sync_at, None);
        assert_eq!(meta.sync_status, None);
    }

    #[test]
    fn bump_revision_with_conn_preserves_unknown_fields() {
        let conn = setup_conn(r#"{"revision":1,"custom":"keep"}"#);

        let meta = bump_revision_with_conn(&conn, 42).expect("updated meta");
        let stored = load_raw_meta_json(&conn);

        assert_eq!(meta.revision, 2);
        assert_eq!(meta.updated_at, Some(42));
        assert_eq!(
            stored,
            json!({"revision": 2, "updated_at": 42, "custom": "keep"})
        );
    }

    #[test]
    fn update_sync_with_conn_preserves_unknown_fields() {
        let conn = setup_conn(r#"{"revision":7,"custom":{"keep":true}}"#);

        let meta = update_sync_with_conn(&conn, 99, "partial").expect("updated meta");
        let stored = load_raw_meta_json(&conn);

        assert_eq!(meta.revision, 7);
        assert_eq!(meta.sync_at, Some(99));
        assert_eq!(meta.sync_status.as_deref(), Some("partial"));
        assert_eq!(
            stored,
            json!({"revision": 7, "sync_at": 99, "sync_status": "partial", "custom": {"keep": true}})
        );
    }

    #[test]
    fn initialize_meta_with_conn_inserts_default_row_once() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute(
            "CREATE TABLE es_meta (id INTEGER PRIMARY KEY, meta_json TEXT NOT NULL)",
            [],
        )
        .expect("create meta table");

        initialize_meta_with_conn(&conn).expect("initialize meta");
        initialize_meta_with_conn(&conn).expect("initialize meta idempotently");

        let stored = load_raw_meta_json(&conn);
        assert_eq!(stored["app_id"], "picofeedr");
        assert!(stored["created_at"].as_i64().is_some());

        let row_count: i64 = conn
            .query_row(sync::COUNT_META_ROWS, [], |row| row.get(0))
            .expect("count meta rows");
        assert_eq!(row_count, 1);
    }
}
