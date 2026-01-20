//! Tag management utilities.

use crate::db::sqlite::SqliteStore;
use crate::error::AppError;
use std::collections::HashSet;

/// Tag manager for tag dictionary operations.
pub struct TagManager<'a> {
    store: &'a SqliteStore,
}

impl<'a> TagManager<'a> {
    /// Creates a new TagManager.
    pub fn new(store: &'a SqliteStore) -> Self {
        Self { store }
    }

    /// Ensures tags exist in the dictionary.
    pub fn ensure_tags(&self, tags: &[String]) -> Result<(), AppError> {
        let mut seen = HashSet::new();
        for tag in tags {
            if seen.insert(tag) {
                self.store.ensure_tag(tag)?;
            }
        }
        Ok(())
    }

    /// Lists tags in alphabetical order.
    pub fn list_tags(&self) -> Result<Vec<String>, AppError> {
        self.store.list_tags()
    }
}
