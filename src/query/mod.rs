//! Query parser for entry filters.

mod date;
mod tag;

use crate::error::AppError;
use crate::time::current_epoch;
use ::time::UtcOffset;

/// Parsed entry query filters.
#[derive(Debug, Clone, Default)]
pub struct EntryQuery {
    /// Optional tag filter expression.
    pub tag_expr: Option<TagExpr>,
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
    /// Filter by public feed id (opaque string).
    Id(String),
    /// Filter by feed title.
    Title(String),
}

/// Tag filter expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagExpr {
    /// Matches entries containing the tag.
    Tag(String),
    /// Negates a nested expression.
    Not(Box<TagExpr>),
    /// Conjunction over nested expressions.
    And(Vec<TagExpr>),
    /// Disjunction over nested expressions.
    Or(Vec<TagExpr>),
}

impl TagExpr {
    /// Returns a stable canonical representation used for hash validation.
    pub(crate) fn canonical(&self) -> String {
        match self {
            TagExpr::Tag(tag) => format!("tag:{}", tag::escape_tag_literal(tag)),
            TagExpr::Not(inner) => format!("not({})", inner.canonical()),
            TagExpr::And(items) => {
                let mut parts = items.iter().map(TagExpr::canonical).collect::<Vec<_>>();
                parts.sort();
                format!("and({})", parts.join(","))
            }
            TagExpr::Or(items) => {
                let mut parts = items.iter().map(TagExpr::canonical).collect::<Vec<_>>();
                parts.sort();
                format!("or({})", parts.join(","))
            }
        }
    }

    /// Returns true when expression tree contains NOT.
    pub(crate) fn contains_not(&self) -> bool {
        match self {
            TagExpr::Tag(_) => false,
            TagExpr::Not(_) => true,
            TagExpr::And(items) | TagExpr::Or(items) => items.iter().any(TagExpr::contains_not),
        }
    }

    /// Counts total AST nodes.
    pub(crate) fn node_count(&self) -> usize {
        match self {
            TagExpr::Tag(_) => 1,
            TagExpr::Not(inner) => 1 + inner.node_count(),
            TagExpr::And(items) | TagExpr::Or(items) => {
                1 + items.iter().map(TagExpr::node_count).sum::<usize>()
            }
        }
    }

    /// Returns maximum AST depth.
    pub(crate) fn max_depth(&self) -> usize {
        match self {
            TagExpr::Tag(_) => 1,
            TagExpr::Not(inner) => 1 + inner.max_depth(),
            TagExpr::And(items) | TagExpr::Or(items) => {
                1 + items.iter().map(TagExpr::max_depth).max().unwrap_or(0)
            }
        }
    }

    /// Returns maximum OR fan-out among all OR nodes.
    pub(crate) fn max_or_fanout(&self) -> usize {
        match self {
            TagExpr::Tag(_) => 0,
            TagExpr::Not(inner) => inner.max_or_fanout(),
            TagExpr::And(items) => items.iter().map(TagExpr::max_or_fanout).max().unwrap_or(0),
            TagExpr::Or(items) => items
                .iter()
                .map(TagExpr::max_or_fanout)
                .max()
                .unwrap_or(0)
                .max(items.len()),
        }
    }

    /// Counts tag literal nodes.
    pub(crate) fn tag_token_count(&self) -> usize {
        match self {
            TagExpr::Tag(_) => 1,
            TagExpr::Not(inner) => inner.tag_token_count(),
            TagExpr::And(items) | TagExpr::Or(items) => {
                items.iter().map(TagExpr::tag_token_count).sum()
            }
        }
    }
}

impl EntryQuery {
    /// Parses a query string into entry filters.
    pub fn parse(raw: Option<&str>, unread_tag: Option<&str>) -> Result<Self, AppError> {
        let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        Self::parse_with_now(raw, unread_tag, current_epoch(), local_offset)
    }

    fn parse_with_now(
        raw: Option<&str>,
        unread_tag: Option<&str>,
        now_epoch_utc: i64,
        local_offset: UtcOffset,
    ) -> Result<Self, AppError> {
        let mut query = EntryQuery::default();
        let raw = match raw {
            Some(raw) => raw.trim(),
            None => "",
        };
        if raw.is_empty() {
            return Ok(query);
        }
        let tokens = tokenize(raw)?;
        let mut tag_terms = Vec::new();
        let mut index = 0usize;
        while index < tokens.len() {
            let token = &tokens[index];
            if token == "unread" {
                if let Some(unread_tag) = unread_tag {
                    tag_terms.push(TagExpr::Tag(unread_tag.to_string()));
                }
                index += 1;
                continue;
            }
            if let Some(value) = token.strip_prefix("tag:") {
                let expr_source = collect_expr_parts(&tokens, &mut index, value, "tag:")?;
                let expr = tag::parse_tag_expr(&expr_source)?;
                tag_terms.push(expr);
                continue;
            }
            if let Some(value) = token.strip_prefix("-tag:") {
                let expr_source = collect_expr_parts(&tokens, &mut index, value, "-tag:")?;
                let inner = tag::parse_minus_tag_expr(&expr_source)?;
                tag_terms.push(TagExpr::Not(Box::new(inner)));
                continue;
            }
            if let Some(value) = token.strip_prefix("feed:") {
                let value = require_value(value, "feed:")?;
                ensure_unique(&query.feed, "feed:")?;
                query.feed = Some(parse_feed_filter(value)?);
                index += 1;
                continue;
            }
            if let Some(value) = token.strip_prefix("title:") {
                let value = require_value(value, "title:")?;
                ensure_unique(&query.title, "title:")?;
                query.title = Some(parse_scalar_value(value)?);
                index += 1;
                continue;
            }
            if let Some(value) = token.strip_prefix("after:") {
                let value = require_value(value, "after:")?;
                ensure_unique(&query.after, "after:")?;
                let value = date::parse_date_or_relative_to_epoch(
                    &parse_scalar_value(value)?,
                    now_epoch_utc,
                    local_offset,
                )?;
                query.after = Some(value);
                index += 1;
                continue;
            }
            if let Some(value) = token.strip_prefix("before:") {
                let value = require_value(value, "before:")?;
                ensure_unique(&query.before, "before:")?;
                let value = date::parse_date_or_relative_to_epoch(
                    &parse_scalar_value(value)?,
                    now_epoch_utc,
                    local_offset,
                )?;
                query.before = Some(value);
                index += 1;
                continue;
            }
            return Err(AppError::invalid_query(format!(
                "Unknown query token: {token}"
            )));
        }

        if !tag_terms.is_empty() {
            // Each `tag:` fragment is already normalized by the tag parser, but once we combine
            // multiple top-level fragments we normalize again to flatten and dedupe across terms.
            let expr = tag::normalize_tag_expr(tag::and_all(tag_terms));
            if tag::has_direct_tag_conflict(&expr) {
                return Err(AppError::invalid_query(
                    "tag: and -tag: cannot target the same tag",
                ));
            }
            query.tag_expr = Some(expr);
        }
        if let (Some(after), Some(before)) = (query.after, query.before)
            && after >= before
        {
            return Err(AppError::invalid_query(
                "after: must be earlier than before",
            ));
        }
        Ok(query)
    }
}

fn collect_expr_parts(
    tokens: &[String],
    index: &mut usize,
    first_value: &str,
    prefix: &str,
) -> Result<String, AppError> {
    let mut parts = Vec::new();
    if !first_value.is_empty() {
        parts.push(first_value.to_string());
    }
    *index += 1;
    while *index < tokens.len() && !is_top_level_token(&tokens[*index]) {
        parts.push(tokens[*index].clone());
        *index += 1;
    }
    if parts.is_empty() {
        return Err(AppError::invalid_query(format!(
            "{prefix} requires a value"
        )));
    }
    Ok(parts.join(" "))
}

fn require_value<'a>(value: &'a str, prefix: &str) -> Result<&'a str, AppError> {
    if value.is_empty() {
        return Err(AppError::invalid_query(format!(
            "{prefix} requires a value"
        )));
    }
    Ok(value)
}

fn ensure_unique<T>(slot: &Option<T>, prefix: &str) -> Result<(), AppError> {
    if slot.is_some() {
        return Err(AppError::invalid_query(format!(
            "{prefix} cannot be specified multiple times"
        )));
    }
    Ok(())
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
                current.push(ch);
            }
            '\\' if in_quotes => {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
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

/// Returns whether a token starts a top-level query directive.
fn is_top_level_token(token: &str) -> bool {
    token == "unread"
        || token.starts_with("tag:")
        || token.starts_with("-tag:")
        || token.starts_with("feed:")
        || token.starts_with("title:")
        || token.starts_with("after:")
        || token.starts_with("before:")
}

/// Parses scalar query values with optional quote escaping.
fn parse_scalar_value(raw: &str) -> Result<String, AppError> {
    if let Some(inner) = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return unescape_quoted(inner);
    }
    if raw.contains('"') {
        return Err(AppError::invalid_query("Invalid quoted value"));
    }
    Ok(raw.to_string())
}

/// Unescapes a quoted string body using `\"` and `\\` rules.
fn unescape_quoted(inner: &str) -> Result<String, AppError> {
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let next = chars
                .next()
                .ok_or_else(|| AppError::invalid_query("Invalid escape sequence"))?;
            match next {
                '\\' | '"' => out.push(next),
                _ => {
                    out.push('\\');
                    out.push(next);
                }
            }
            continue;
        }
        out.push(ch);
    }
    Ok(out)
}

/// Parses a feed filter from a feed token value.
fn parse_feed_filter(value: &str) -> Result<FeedFilter, AppError> {
    if value.starts_with('"') && value.ends_with('"') {
        return Ok(FeedFilter::Title(parse_scalar_value(value)?));
    }
    Ok(FeedFilter::Id(parse_scalar_value(value)?))
}

#[cfg(test)]
mod tests {
    use super::{EntryQuery, FeedFilter};
    use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

    fn fixed_now_utc() -> i64 {
        OffsetDateTime::new_utc(
            Date::from_calendar_date(2026, Month::February, 26).expect("date"),
            Time::from_hms(3, 0, 0).expect("time"),
        )
        .unix_timestamp()
    }

    fn fixed_jst() -> UtcOffset {
        UtcOffset::from_hms(9, 0, 0).expect("offset")
    }

    fn local_midnight_epoch(year: i32, month: Month, day: u8, offset: UtcOffset) -> i64 {
        PrimitiveDateTime::new(
            Date::from_calendar_date(year, month, day).expect("date"),
            Time::MIDNIGHT,
        )
        .assume_offset(offset)
        .unix_timestamp()
    }

    #[test]
    fn parse_feed_id_query() {
        let query = EntryQuery::parse(Some("feed:123"), Some("unread")).expect("query");
        assert_eq!(query.feed, Some(FeedFilter::Id("123".to_string())));
    }

    #[test]
    fn parse_feed_title_query() {
        let query =
            EntryQuery::parse(Some("feed:\"Example Feed\""), Some("unread")).expect("query");
        assert_eq!(
            query.feed,
            Some(FeedFilter::Title("Example Feed".to_string()))
        );
    }

    #[test]
    fn parse_title_query() {
        let query = EntryQuery::parse(Some("title:\"First\""), Some("unread")).expect("query");
        assert_eq!(query.title.as_deref(), Some("First"));
    }

    #[test]
    fn rejects_duplicate_feed_tokens() {
        let error = EntryQuery::parse(Some("feed:1 feed:2"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_duplicate_title_tokens() {
        let error = EntryQuery::parse(Some("title:foo title:bar"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn parse_date_bounds() {
        let query = EntryQuery::parse(Some("after:2026-01-01 before:2026-01-02"), Some("unread"))
            .expect("query");
        assert!(query.after.is_some());
        assert!(query.before.is_some());
        assert!(query.after.unwrap() < query.before.unwrap());
    }

    #[test]
    fn parse_relative_date_bounds() {
        let query = EntryQuery::parse_with_now(
            Some("after:1m before:3d"),
            Some("unread"),
            fixed_now_utc(),
            fixed_jst(),
        )
        .expect("query");
        assert_eq!(
            query.after,
            Some(local_midnight_epoch(2026, Month::January, 26, fixed_jst()))
        );
        assert_eq!(
            query.before,
            Some(local_midnight_epoch(2026, Month::February, 23, fixed_jst()))
        );
    }

    #[test]
    fn parse_relative_week_date_bounds() {
        let query = EntryQuery::parse_with_now(
            Some("after:1w before:3d"),
            Some("unread"),
            fixed_now_utc(),
            fixed_jst(),
        )
        .expect("query");
        assert_eq!(
            query.after,
            Some(local_midnight_epoch(2026, Month::February, 19, fixed_jst()))
        );
        assert_eq!(
            query.before,
            Some(local_midnight_epoch(2026, Month::February, 23, fixed_jst()))
        );
    }

    #[test]
    fn parse_mixed_absolute_and_relative_date_bounds() {
        let query = EntryQuery::parse_with_now(
            Some("after:3m before:2026-01-01"),
            Some("unread"),
            fixed_now_utc(),
            fixed_jst(),
        )
        .expect("query");
        assert_eq!(
            query.after,
            Some(local_midnight_epoch(2025, Month::November, 26, fixed_jst()))
        );
        assert_eq!(
            query.before,
            Some(local_midnight_epoch(2026, Month::January, 1, fixed_jst()))
        );
    }

    #[test]
    fn parse_absolute_date_bounds_use_local_midnight() {
        let query = EntryQuery::parse_with_now(
            Some("after:2026-01-01 before:2026-01-02"),
            Some("unread"),
            fixed_now_utc(),
            fixed_jst(),
        )
        .expect("query");
        assert_eq!(
            query.after,
            Some(local_midnight_epoch(2026, Month::January, 1, fixed_jst()))
        );
        assert_eq!(
            query.before,
            Some(local_midnight_epoch(2026, Month::January, 2, fixed_jst()))
        );
    }

    #[test]
    fn parse_zero_relative_date_units_as_same_anchor() {
        let after = EntryQuery::parse_with_now(
            Some("after:0d"),
            Some("unread"),
            fixed_now_utc(),
            fixed_jst(),
        )
        .expect("query");
        let before = EntryQuery::parse_with_now(
            Some("before:0w"),
            Some("unread"),
            fixed_now_utc(),
            fixed_jst(),
        )
        .expect("query");
        let anchor = local_midnight_epoch(2026, Month::February, 26, fixed_jst());
        assert_eq!(after.after, Some(anchor));
        assert_eq!(before.before, Some(anchor));
    }

    #[test]
    fn rejects_invalid_relative_date_unit() {
        let error = EntryQuery::parse_with_now(
            Some("after:3x"),
            Some("unread"),
            fixed_now_utc(),
            fixed_jst(),
        )
        .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_overflow_relative_duration() {
        let error = EntryQuery::parse_with_now(
            Some("after:2147483648y"),
            Some("unread"),
            fixed_now_utc(),
            fixed_jst(),
        )
        .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_duplicate_after_tokens() {
        let error = EntryQuery::parse(Some("after:2026-01-01 after:2026-01-02"), Some("unread"))
            .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_duplicate_before_tokens() {
        let error = EntryQuery::parse(Some("before:2026-01-01 before:2026-01-02"), Some("unread"))
            .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_invalid_date_range() {
        let error = EntryQuery::parse(Some("after:2026-01-02 before:2026-01-02"), Some("unread"))
            .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_invalid_relative_date_range() {
        let error = EntryQuery::parse_with_now(
            Some("after:0d before:1y"),
            Some("unread"),
            fixed_now_utc(),
            fixed_jst(),
        )
        .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_unknown_tokens() {
        let error = EntryQuery::parse(Some("oops"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn unread_keyword_is_noop_when_unread_management_is_disabled() {
        let query = EntryQuery::parse(Some("unread"), None).expect("query");
        assert!(query.tag_expr.is_none());
    }
}
