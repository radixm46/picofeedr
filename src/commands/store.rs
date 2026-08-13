use picofeedr::config;
use picofeedr::db;
use picofeedr::error::AppError;
use tracing::debug;

pub(super) fn with_store<T>(
    config: &config::AppConfig,
    f: impl FnOnce(&mut db::sqlite::SqliteStore) -> Result<T, AppError>,
) -> Result<T, AppError> {
    debug!(
        db_path = ?config.database_path(),
        feeds_path = ?config.feeds_source(),
        "loaded configuration"
    );
    let mut store = db::sqlite::SqliteStore::open(config.database_path())?;
    store.migrate()?;
    f(&mut store)
}
