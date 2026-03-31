//! Tag management utilities.

use crate::db::sqlite::SqliteStore;
use crate::error::AppError;
use crate::string_set::{
    dedupe_strings_preserve_order, duplicated_strings_preserve_order, merge_unique_strings,
    split_csv_trimmed_unique,
};

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

/// Parses a comma-separated tag list from CLI input.
pub fn parse_tag_csv(raw: Option<&str>) -> Vec<String> {
    split_csv_trimmed_unique(raw)
}

/// Deduplicates tag names while preserving first-seen order.
pub(crate) fn dedupe_tag_names(values: impl IntoIterator<Item = String>) -> Vec<String> {
    dedupe_strings_preserve_order(values)
}

/// Merges tag lists while preserving order and uniqueness.
pub(crate) fn merge_tag_names(base: &[String], extra: &[String]) -> Vec<String> {
    merge_unique_strings(base, extra)
}

/// Returns duplicated tag names while preserving first duplicate order.
pub(crate) fn duplicated_tag_names(values: &[String]) -> Vec<String> {
    duplicated_strings_preserve_order(values)
}
