//! Metadata helpers backed by `es_meta.meta_json`.

use crate::error::AppError;
use rusqlite::{Connection, params};
use serde_json::{Map, Value};

/// Database-wide status metadata used by `status` and list snapshots.
#[derive(Debug, Clone)]
pub struct SystemMeta {
    /// Monotonic revision incremented after successful write commands.
    pub db_revision: i64,
    /// Epoch seconds of the latest successful write command.
    pub last_write_at: Option<i64>,
    /// Epoch seconds of the latest successful sync command.
    pub last_sync_at: Option<i64>,
    /// Status of the latest successful sync command.
    pub last_sync_status: Option<String>,
}

/// Loads system metadata from `es_meta.meta_json`.
pub(crate) fn read_system_meta_with_conn(conn: &Connection) -> Result<SystemMeta, AppError> {
    let meta_json = load_meta_json_text_with_conn(conn)?;
    parse_system_meta(&meta_json)
}

/// Increments `db_revision` and updates `last_write_at`.
pub(crate) fn bump_system_revision_with_conn(
    conn: &Connection,
    now: i64,
) -> Result<SystemMeta, AppError> {
    let meta_json = load_meta_json_text_with_conn(conn)?;
    let mut object = parse_meta_object(meta_json)?;
    let revision = object
        .get("db_revision")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .saturating_add(1);
    object.insert("db_revision".to_string(), Value::from(revision));
    object.insert("last_write_at".to_string(), Value::from(now));
    write_meta_json_object_with_conn(conn, object)
}

/// Updates `last_sync_at` and `last_sync_status`.
pub(crate) fn update_last_sync_with_conn(
    conn: &Connection,
    now: i64,
    status: &str,
) -> Result<SystemMeta, AppError> {
    let meta_json = load_meta_json_text_with_conn(conn)?;
    let mut object = parse_meta_object(meta_json)?;
    object.insert("last_sync_at".to_string(), Value::from(now));
    object.insert("last_sync_status".to_string(), Value::from(status));
    write_meta_json_object_with_conn(conn, object)
}

/// Reads raw `meta_json` text from `es_meta`.
fn load_meta_json_text_with_conn(conn: &Connection) -> Result<String, AppError> {
    conn.query_row("SELECT meta_json FROM es_meta WHERE id = 1", [], |row| {
        row.get(0)
    })
    .map_err(AppError::from)
}

/// Parses `meta_json` text into a JSON object.
fn parse_meta_object(meta_json: String) -> Result<Map<String, Value>, AppError> {
    match serde_json::from_str::<Value>(&meta_json)? {
        Value::Object(object) => Ok(object),
        _ => Ok(Map::new()),
    }
}

/// Writes a JSON object back into `es_meta.meta_json`.
fn write_meta_json_object_with_conn(
    conn: &Connection,
    object: Map<String, Value>,
) -> Result<SystemMeta, AppError> {
    let meta_json = serde_json::to_string(&Value::Object(object.clone()))?;
    conn.execute(
        "UPDATE es_meta SET meta_json = ?1 WHERE id = 1",
        params![meta_json],
    )?;
    parse_system_meta_object(object)
}

/// Parses status fields from `meta_json` text.
fn parse_system_meta(meta_json: &str) -> Result<SystemMeta, AppError> {
    let object = parse_meta_object(meta_json.to_string())?;
    parse_system_meta_object(object)
}

/// Parses status fields from JSON object.
fn parse_system_meta_object(object: Map<String, Value>) -> Result<SystemMeta, AppError> {
    let db_revision = object
        .get("db_revision")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let last_write_at = object.get("last_write_at").and_then(Value::as_i64);
    let last_sync_at = object.get("last_sync_at").and_then(Value::as_i64);
    let last_sync_status = object
        .get("last_sync_status")
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(SystemMeta {
        db_revision,
        last_write_at,
        last_sync_at,
        last_sync_status,
    })
}
