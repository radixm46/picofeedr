//! Query parser for entry filters.

use crate::error::AppError;
use ::time::{Date, Month, PrimitiveDateTime, Time};

/// Parsed entry query filters.
#[derive(Debug, Clone, Default)]
pub struct EntryQuery {
    /// Tags that must be present.
    pub include_tags: Vec<String>,
    /// Tags that must be absent.
    pub exclude_tags: Vec<String>,
    /// Feed filter.
    pub feed: Option<FeedFilter>,
    /// Entry title keyword.
    pub title: Option<String>,
    /// Lower date bound (inclusive).
    pub after: Option<i64>,
    /// Upper date bound (exclusive).
    pub before: Option<i64>,
}

/// Feed filter variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedFilter {
    /// Filter by feed id.
    Id(i64),
    /// Filter by feed title.
    Title(String),
}

impl EntryQuery {
    /// Parses a query string into entry filters.
    pub fn parse(raw: Option<&str>, unread_tag: &str) -> Result<Self, AppError> {
        let mut query = EntryQuery::default();
        let raw = match raw {
            Some(raw) => raw.trim(),
            None => "",
        };
        if raw.is_empty() {
            return Ok(query);
        }
        let tokens = tokenize(raw)?;
        for token in tokens {
            if token == "unread" {
                query.include_tags.push(unread_tag.to_string());
                continue;
            }
            if let Some(tag) = token.strip_prefix("tag:") {
                if tag.is_empty() {
                    return Err(AppError::invalid_query("tag: requires a value"));
                }
                query.include_tags.push(tag.to_string());
                continue;
            }
            if let Some(tag) = token.strip_prefix("-tag:") {
                if tag.is_empty() {
                    return Err(AppError::invalid_query("-tag: requires a value"));
                }
                query.exclude_tags.push(tag.to_string());
                continue;
            }
            if let Some(value) = token.strip_prefix("feed:") {
                if value.is_empty() {
                    return Err(AppError::invalid_query("feed: requires a value"));
                }
                if query.feed.is_some() {
                    return Err(AppError::invalid_query(
                        "feed: cannot be specified multiple times",
                    ));
                }
                query.feed = Some(parse_feed_filter(value)?);
                continue;
            }
            if let Some(value) = token.strip_prefix("title:") {
                if value.is_empty() {
                    return Err(AppError::invalid_query("title: requires a value"));
                }
                if query.title.is_some() {
                    return Err(AppError::invalid_query(
                        "title: cannot be specified multiple times",
                    ));
                }
                query.title = Some(value.to_string());
                continue;
            }
            if let Some(value) = token.strip_prefix("after:") {
                if value.is_empty() {
                    return Err(AppError::invalid_query("after: requires a value"));
                }
                if query.after.is_some() {
                    return Err(AppError::invalid_query(
                        "after: cannot be specified multiple times",
                    ));
                }
                query.after = Some(parse_date_to_epoch(value)?);
                continue;
            }
            if let Some(value) = token.strip_prefix("before:") {
                if value.is_empty() {
                    return Err(AppError::invalid_query("before: requires a value"));
                }
                if query.before.is_some() {
                    return Err(AppError::invalid_query(
                        "before: cannot be specified multiple times",
                    ));
                }
                query.before = Some(parse_date_to_epoch(value)?);
                continue;
            }
            return Err(AppError::invalid_query(format!(
                "Unknown query token: {token}"
            )));
        }
        Ok(query)
    }
}

/// Tokenizes query text while honoring quoted segments.
///
/// Quoted segments are delimited by `"` and may contain escaped `\"` and `\\`.
fn tokenize(raw: &str) -> Result<Vec<String>, AppError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = raw.chars().peekable();
    let mut in_quotes = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            '\\' if in_quotes => {
                if let Some(next) = chars.next() {
                    match next {
                        '"' | '\\' => current.push(next),
                        _ => {
                            current.push('\\');
                            current.push(next);
                        }
                    }
                } else {
                    current.push('\\');
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if in_quotes {
        return Err(AppError::invalid_query("Unclosed quote in query"));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

/// Parses a feed filter from a feed token value.
fn parse_feed_filter(value: &str) -> Result<FeedFilter, AppError> {
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        let id = value
            .parse::<i64>()
            .map_err(|_| AppError::invalid_query("feed: requires a valid id"))?;
        return Ok(FeedFilter::Id(id));
    }
    Ok(FeedFilter::Title(value.to_string()))
}

/// Parses an ISO date (YYYY-MM-DD) to epoch seconds at UTC midnight.
fn parse_date_to_epoch(value: &str) -> Result<i64, AppError> {
    let mut parts = value.split('-');
    let year = parts
        .next()
        .ok_or_else(|| AppError::invalid_query("Invalid date"))?
        .parse::<i32>()
        .map_err(|_| AppError::invalid_query("Invalid date"))?;
    let month = parts
        .next()
        .ok_or_else(|| AppError::invalid_query("Invalid date"))?
        .parse::<u8>()
        .map_err(|_| AppError::invalid_query("Invalid date"))?;
    let day = parts
        .next()
        .ok_or_else(|| AppError::invalid_query("Invalid date"))?
        .parse::<u8>()
        .map_err(|_| AppError::invalid_query("Invalid date"))?;
    if parts.next().is_some() {
        return Err(AppError::invalid_query("Invalid date"));
    }
    let month = Month::try_from(month).map_err(|_| AppError::invalid_query("Invalid date"))?;
    let date = Date::from_calendar_date(year, month, day)
        .map_err(|_| AppError::invalid_query("Invalid date"))?;
    let datetime = PrimitiveDateTime::new(date, Time::MIDNIGHT);
    Ok(datetime.assume_utc().unix_timestamp())
}

#[cfg(test)]
mod tests {
    use super::{EntryQuery, FeedFilter};

    #[test]
    fn parse_tag_filters() {
        let query = EntryQuery::parse(Some("unread tag:tech -tag:misc"), "unread").expect("query");
        assert!(query.include_tags.contains(&"unread".to_string()));
        assert!(query.include_tags.contains(&"tech".to_string()));
        assert!(query.exclude_tags.contains(&"misc".to_string()));
    }

    #[test]
    fn parse_feed_id_query() {
        let query = EntryQuery::parse(Some("feed:123"), "unread").expect("query");
        assert_eq!(query.feed, Some(FeedFilter::Id(123)));
    }

    #[test]
    fn parse_feed_title_query() {
        let query = EntryQuery::parse(Some("feed:\"Example Feed\""), "unread").expect("query");
        assert_eq!(
            query.feed,
            Some(FeedFilter::Title("Example Feed".to_string()))
        );
    }

    #[test]
    fn parse_title_query() {
        let query = EntryQuery::parse(Some("title:\"First\""), "unread").expect("query");
        assert_eq!(query.title.as_deref(), Some("First"));
    }

    #[test]
    fn rejects_duplicate_feed_tokens() {
        let error = EntryQuery::parse(Some("feed:1 feed:2"), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_duplicate_title_tokens() {
        let error = EntryQuery::parse(Some("title:foo title:bar"), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn parse_date_bounds() {
        let query =
            EntryQuery::parse(Some("after:2026-01-01 before:2026-01-02"), "unread").expect("query");
        assert!(query.after.is_some());
        assert!(query.before.is_some());
        assert!(query.after.unwrap() < query.before.unwrap());
    }

    #[test]
    fn rejects_duplicate_after_tokens() {
        let error =
            EntryQuery::parse(Some("after:2026-01-01 after:2026-01-02"), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_duplicate_before_tokens() {
        let error =
            EntryQuery::parse(Some("before:2026-01-01 before:2026-01-02"), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_unknown_tokens() {
        let error = EntryQuery::parse(Some("oops"), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }
}
