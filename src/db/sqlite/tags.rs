//! Tag queries for SQLite store.

use crate::error::AppError;
use rusqlite::{Connection, params};

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
