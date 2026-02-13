//! SQLite schema assets.

/// Current schema version for SQLite.
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 1;

/// Full DDL used to initialize SQLite database.
pub(crate) const V1_SCHEMA_SQL: &str = include_str!("v1.sql");
