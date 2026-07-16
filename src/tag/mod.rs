//! Tag management utilities.

use crate::error::{AppError, error_details};
use crate::string_set::split_csv_trimmed_unique;
use serde_json::Value as JsonValue;

/// Maximum number of Unicode scalar values in a tag name.
pub(crate) const MAX_TAG_NAME_CHARS: usize = 64;

/// Reason that a tag name violates the shared tag contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagNameViolation {
    /// Tag names must contain at least one character.
    Empty,
    /// Tag names use canonical names without surrounding whitespace.
    SurroundingWhitespace,
    /// Comma is reserved by comma-separated CLI tag input.
    ReservedComma,
    /// Control characters are not valid tag content.
    ControlCharacter,
    /// Tag name exceeds the supported length.
    TooLong,
}

impl TagNameViolation {
    /// Returns the user-facing validation message.
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Empty => "tag name must not be empty",
            Self::SurroundingWhitespace => {
                "tag name must not contain leading or trailing whitespace"
            }
            Self::ReservedComma => "tag name must not contain reserved comma",
            Self::ControlCharacter => "tag name must not contain control characters",
            Self::TooLong => "tag name must not exceed 64 characters",
        }
    }

    /// Returns the machine-readable remediation hint.
    pub(crate) fn hint(self) -> &'static str {
        match self {
            Self::Empty => "set_a_non_empty_tag_name",
            Self::SurroundingWhitespace => "remove_surrounding_whitespace",
            Self::ReservedComma => "remove_reserved_comma",
            Self::ControlCharacter => "remove_control_characters",
            Self::TooLong => "shorten_tag_name",
        }
    }
}

/// Validates a canonical tag name shared by configuration and query inputs.
pub(crate) fn validate_tag_name(tag: &str) -> Result<(), TagNameViolation> {
    if tag.is_empty() {
        return Err(TagNameViolation::Empty);
    }
    if tag.trim() != tag {
        return Err(TagNameViolation::SurroundingWhitespace);
    }
    if tag.chars().count() > MAX_TAG_NAME_CHARS {
        return Err(TagNameViolation::TooLong);
    }
    if tag.contains(',') {
        return Err(TagNameViolation::ReservedComma);
    }
    if tag.chars().any(char::is_control) {
        return Err(TagNameViolation::ControlCharacter);
    }
    Ok(())
}

/// Builds the structured user-input error for an invalid tag name.
pub(crate) fn invalid_tag_name_error(
    value: impl Into<String>,
    field: &'static str,
    violation: TagNameViolation,
) -> AppError {
    AppError::invalid_query_with_details(
        violation.message(),
        error_details([
            ("kind", JsonValue::from("invalid_tag_name")),
            ("field", JsonValue::from(field)),
            ("value", JsonValue::from(value.into())),
            ("hint", JsonValue::from(violation.hint())),
        ]),
    )
}

/// Parses and normalizes a comma-separated tag list from CLI input.
pub fn parse_tag_csv(raw: Option<&str>) -> Vec<String> {
    split_csv_trimmed_unique(raw)
}

#[cfg(test)]
mod tests {
    use super::validate_tag_name;

    #[test]
    fn rejects_empty_tag_name() {
        assert!(validate_tag_name("").is_err());
    }
}
