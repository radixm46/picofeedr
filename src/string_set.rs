use std::collections::HashSet;

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

/// Splits a comma-separated list, trims whitespace, drops empty parts, and deduplicates.
pub fn split_csv_trimmed_unique(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    dedupe_strings_preserve_order(
        raw.split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    )
}

/// Returns duplicated values while preserving first duplicate order.
pub(crate) fn duplicated_strings_preserve_order(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut duplicates = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        if !seen.insert(value.clone()) && duplicates.insert(value.clone()) {
            result.push(value.clone());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        dedupe_strings_preserve_order, duplicated_strings_preserve_order, merge_unique_strings,
        split_csv_trimmed_unique,
    };

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

    #[test]
    fn split_csv_trimmed_unique_drops_empty_values() {
        let tags = split_csv_trimmed_unique(Some(" tech, rust , ,tech,cli "));

        assert_eq!(tags, vec!["tech", "rust", "cli"]);
    }

    #[test]
    fn duplicated_strings_preserve_order_reports_each_duplicate_once() {
        let values = vec![
            "tech".to_string(),
            "rust".to_string(),
            "tech".to_string(),
            "cli".to_string(),
            "rust".to_string(),
            "rust".to_string(),
        ];

        let duplicates = duplicated_strings_preserve_order(&values);

        assert_eq!(duplicates, vec!["tech", "rust"]);
    }
}
