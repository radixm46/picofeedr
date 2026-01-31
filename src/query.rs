//! Query parser for tag-based filters.

use crate::error::AppError;

/// Parsed tag filter query.
#[derive(Debug, Clone, Default)]
pub struct TagQuery {
    /// Tags that must be present.
    pub include: Vec<String>,
    /// Tags that must be absent.
    pub exclude: Vec<String>,
}

impl TagQuery {
    /// Parses a query string into tag filters.
    pub fn parse(raw: Option<&str>, unread_tag: &str) -> Result<Self, AppError> {
        let mut query = TagQuery::default();
        let raw = match raw {
            Some(raw) => raw.trim(),
            None => "",
        };
        if raw.is_empty() {
            return Ok(query);
        }
        for token in raw.split_whitespace() {
            if token == "unread" {
                query.include.push(unread_tag.to_string());
                continue;
            }
            if token == "star" || token == "starred" {
                query.include.push("star".to_string());
                continue;
            }
            if let Some(tag) = token.strip_prefix("tag:") {
                if tag.is_empty() {
                    return Err(AppError::invalid_query("tag: requires a value"));
                }
                query.include.push(tag.to_string());
                continue;
            }
            if let Some(tag) = token.strip_prefix("-tag:") {
                if tag.is_empty() {
                    return Err(AppError::invalid_query("-tag: requires a value"));
                }
                query.exclude.push(tag.to_string());
                continue;
            }
            return Err(AppError::invalid_query(format!(
                "Unknown query token: {token}"
            )));
        }
        Ok(query)
    }
}
