//! Tag queries for SQLite store.

use crate::error::AppError;
use rusqlite::{Connection, params, params_from_iter};
use std::collections::{HashMap, HashSet};

/// Lists all tags ordered by name.
pub(crate) fn list_tags_with_conn(conn: &Connection) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare("SELECT name FROM tags ORDER BY name ASC")?;
    let tags = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(tags)
}

/// Inserts a tag if it does not exist using a provided connection.
pub(crate) fn ensure_tag_with_conn(conn: &Connection, name: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
        params![name],
    )?;
    Ok(())
}

/// Ensures tags exist and returns a name->id map for the provided list.
pub(crate) fn ensure_tag_ids_with_conn(
    conn: &Connection,
    tags: &[String],
) -> Result<HashMap<String, i64>, AppError> {
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for tag in tags {
        if seen.insert(tag.clone()) {
            unique.push(tag.clone());
        }
    }
    for tag in &unique {
        ensure_tag_with_conn(conn, tag)?;
    }
    lookup_tag_ids_with_conn(conn, &unique)
}

/// Returns a name->id map for tags that exist in the database.
pub(crate) fn lookup_tag_ids_with_conn(
    conn: &Connection,
    tags: &[String],
) -> Result<HashMap<String, i64>, AppError> {
    if tags.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat_n("?", tags.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT id, name FROM tags WHERE name IN ({placeholders})");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(tags.iter()))?;
    let mut map = HashMap::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        map.insert(name, id);
    }
    Ok(map)
}
