//! SQLite schema assets.

/// One forward-only SQLite schema migration step.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MigrationStep {
    /// Source schema version before applying this step.
    pub from: i64,
    /// Target schema version after applying this step.
    pub to: i64,
    /// SQL batch executed for this migration step.
    pub sql: &'static str,
}

/// Current schema version for SQLite.
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 1;

/// Schema version produced by bootstrapping a brand-new SQLite database.
pub(crate) const BOOTSTRAP_SCHEMA_VERSION: i64 = 1;

/// Full DDL used to initialize a brand-new SQLite database.
pub(crate) const BOOTSTRAP_SCHEMA_SQL: &str = include_str!("v1.sql");

/// Forward-only migration steps applied after bootstrap.
pub(crate) const MIGRATIONS: &[MigrationStep] = &[];
