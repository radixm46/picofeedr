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

    /// Lists tags in alphabetical order.
    pub fn list_tags(&self) -> Result<Vec<String>, AppError> {
        self.store.list_tags()
    }
}

/// Deduplicates strings while preserving first-seen order.
pub(crate) fn dedupe_strings_preserve_order(
    values: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}

/// Merges two string lists while preserving order and uniqueness.
pub(crate) fn merge_unique_strings(base: &[String], extra: &[String]) -> Vec<String> {
    dedupe_strings_preserve_order(base.iter().chain(extra.iter()).cloned())
}

#[cfg(test)]
mod tests {
    use super::{dedupe_strings_preserve_order, merge_unique_strings};

    #[test]
    fn dedupe_strings_preserve_first_seen_order() {
        let values = vec![
            "tech".to_string(),
            "rust".to_string(),
            "tech".to_string(),
            "cli".to_string(),
            "rust".to_string(),
        ];

        let deduped = dedupe_strings_preserve_order(values);

        assert_eq!(deduped, vec!["tech", "rust", "cli"]);
    }

    #[test]
    fn merge_unique_strings_keeps_base_order_then_new_values() {
        let base = vec!["tech".to_string(), "rust".to_string()];
        let extra = vec![
            "rust".to_string(),
            "cli".to_string(),
            "tech".to_string(),
            "feed".to_string(),
        ];

        let merged = merge_unique_strings(&base, &extra);

        assert_eq!(merged, vec!["tech", "rust", "cli", "feed"]);
    }
}
