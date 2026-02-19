//! Sync and metadata SQL queries.

/// Loads metadata json payload.
pub(crate) const SELECT_META_JSON: &str = "SELECT meta_json FROM es_meta WHERE id = 1";

/// Persists metadata json payload.
pub(crate) const UPDATE_META_JSON: &str = "UPDATE es_meta SET meta_json = ?1 WHERE id = 1";

/// Returns number of rows in metadata table.
pub(crate) const COUNT_META_ROWS: &str = "SELECT COUNT(1) FROM es_meta";

/// Inserts initial metadata row.
pub(crate) const INSERT_META_ROW: &str = "INSERT INTO es_meta (id, meta_json) VALUES (1, ?1)";
