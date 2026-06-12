//! Tag management utilities.

use crate::string_set::split_csv_trimmed_unique;

/// Parses a comma-separated tag list from CLI input.
pub fn parse_tag_csv(raw: Option<&str>) -> Vec<String> {
    split_csv_trimmed_unique(raw)
}
