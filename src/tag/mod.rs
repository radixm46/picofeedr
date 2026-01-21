//! Tag management utilities.

use crate::db::sqlite::SqliteStore;
use crate::error::AppError;

/// Tag manager for tag dictionary operations.
pub struct TagManager<'a> {
    store: &'a SqliteStore,
}

impl<'a> TagManager<'a> {
    /// Creates a new TagManager.
    pub fn new(store: &'a SqliteStore) -> Self {
        Self { store }
    }

    /// Lists tags in alphabetical order.
    pub fn list_tags(&self) -> Result<Vec<String>, AppError> {
        self.store.list_tags()
    }
}
