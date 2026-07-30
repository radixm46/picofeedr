//! Query parser for entry filters.

mod date;
mod expr;

use crate::error::{AppError, error_details};
use crate::time::current_epoch;
use ::time::UtcOffset;
use serde_json::Value as JsonValue;

const MAX_TITLE_TERMS: usize = 32;

/// Parsed entry query filters.
#[derive(Debug, Clone, Default)]
pub struct EntryQuery {
    /// Optional tag filter expression.
    pub tag_expr: Option<TagExpr>,
    /// Feed filter.
    pub feed: Option<FeedFilter>,
    /// Positive title search terms.
    pub title_terms: Vec<String>,
    /// Negative title search terms.
    pub negated_title_terms: Vec<String>,
    /// Positive title term boolean groups.
    pub term_groups: Vec<TermExpr>,
    /// Negative title term boolean groups.
    pub negated_term_groups: Vec<TermExpr>,
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

/// Title term boolean expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermExpr {
    /// Matches entries whose title contains the term.
    Term(String),
    /// Negates a nested expression.
    Not(Box<TermExpr>),
    /// Conjunction over nested expressions.
    And(Vec<TermExpr>),
    /// Disjunction over nested expressions.
    Or(Vec<TermExpr>),
}

impl TagExpr {
    /// Returns a stable canonical representation used for hash validation.
    pub(crate) fn canonical(&self) -> String {
        match self {
            TagExpr::Tag(tag) => format!("tag:{}", expr::escape_tag_literal(tag)),
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

impl TermExpr {
    /// Returns a stable canonical representation used for hash validation.
    pub(crate) fn canonical(&self) -> String {
        match self {
            TermExpr::Term(term) => {
                format!(
                    "term:{}",
                    serde_json::to_string(term).expect("serialize term literal")
                )
            }
            TermExpr::Not(inner) => format!("not({})", inner.canonical()),
            TermExpr::And(items) => {
                let mut parts = items.iter().map(TermExpr::canonical).collect::<Vec<_>>();
                parts.sort();
                format!("and({})", parts.join(","))
            }
            TermExpr::Or(items) => {
                let mut parts = items.iter().map(TermExpr::canonical).collect::<Vec<_>>();
                parts.sort();
                format!("or({})", parts.join(","))
            }
        }
    }

    /// Returns true when expression tree contains NOT.
    pub(crate) fn contains_not(&self) -> bool {
        match self {
            TermExpr::Term(_) => false,
            TermExpr::Not(_) => true,
            TermExpr::And(items) | TermExpr::Or(items) => items.iter().any(TermExpr::contains_not),
        }
    }

    /// Returns maximum AST depth.
    pub(crate) fn max_depth(&self) -> usize {
        match self {
            TermExpr::Term(_) => 1,
            TermExpr::Not(inner) => 1 + inner.max_depth(),
            TermExpr::And(items) | TermExpr::Or(items) => {
                1 + items.iter().map(TermExpr::max_depth).max().unwrap_or(0)
            }
        }
    }

    /// Counts term literal nodes.
    pub(crate) fn term_count(&self) -> usize {
        match self {
            TermExpr::Term(_) => 1,
            TermExpr::Not(inner) => inner.term_count(),
            TermExpr::And(items) | TermExpr::Or(items) => {
                items.iter().map(TermExpr::term_count).sum()
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
        let mut tag_seen = false;
        let mut minus_tag_seen = false;
        let mut index = 0usize;
        while index < tokens.len() {
            let item = &tokens[index];
            let token = &item.value;
            debug_assert!(index == 0 || item.whitespace_before);
            if token == "unread" {
                if let Some(unread_tag) = unread_tag {
                    tag_terms.push(TagExpr::Tag(unread_tag.to_string()));
                }
                index += 1;
                continue;
            }
            if let Some(value) = token.strip_prefix("tag:") {
                require_value(value, "tag:")?;
                if tag_seen {
                    return Err(duplicate_query_filter_error(
                        "tag:",
                        "merge_into_single_tag_expression",
                    ));
                }
                tag_seen = true;
                let expr = expr::parse_tag_expr(item.expression_after_prefix("tag:")?)?;
                tag_terms.push(expr);
                index += 1;
                continue;
            }
            if let Some(value) = token.strip_prefix("-tag:") {
                require_value(value, "-tag:")?;
                if minus_tag_seen {
                    return Err(duplicate_query_filter_error(
                        "-tag:",
                        "merge_into_single_minus_tag_expression",
                    ));
                }
                minus_tag_seen = true;
                let inner = expr::parse_minus_tag_expr(item.expression_after_prefix("-tag:")?)?;
                tag_terms.push(TagExpr::Not(Box::new(inner)));
                index += 1;
                continue;
            }
            if let Some(value) = token.strip_prefix("feed:") {
                let value = require_value(value, "feed:")?;
                ensure_unique(&query.feed, "feed:")?;
                query.feed = Some(parse_feed_filter(value)?);
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
            if token.strip_prefix("-(").is_some() {
                let expr = expr::parse_minus_term_expr(item.expression_after_prefix("-")?)?;
                match expr {
                    TermExpr::Term(term) => query.negated_title_terms.push(term),
                    expr => query.negated_term_groups.push(expr),
                }
                index += 1;
                continue;
            }
            if token.starts_with('(') {
                let expr = expr::parse_term_expr(item.expression_tokens.clone())?;
                match expr {
                    TermExpr::Term(term) => query.title_terms.push(term),
                    expr => query.term_groups.push(expr),
                }
                index += 1;
                continue;
            }
            if is_bare_operator_token(token) {
                return Err(bare_operator_token_error(token));
            }
            if token.starts_with('-') {
                query
                    .negated_title_terms
                    .push(parse_negated_title_term(token)?);
                index += 1;
                continue;
            }
            if token.contains(':') && !is_quoted_scalar(token) {
                return Err(unknown_filter_prefix_error(token));
            }
            query.title_terms.push(parse_title_term(token)?);
            index += 1;
        }
        validate_title_terms(&query)?;

        if !tag_terms.is_empty() {
            // Each `tag:` fragment is already normalized by the tag parser, but once we combine
            // multiple top-level fragments we normalize again to flatten and dedupe across terms.
            let expr = expr::normalize_tag_expr(expr::and_all(tag_terms));
            if expr::has_direct_tag_conflict(&expr) {
                return Err(AppError::invalid_query(
                    "tag expression requires and excludes the same tag",
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
        return Err(duplicate_query_filter_error(
            prefix,
            "remove_duplicate_filter",
        ));
    }
    Ok(())
}

fn duplicate_query_filter_error(prefix: &str, hint: &str) -> AppError {
    AppError::invalid_query_with_details(
        format!("{prefix} cannot be specified multiple times"),
        error_details([
            ("kind", JsonValue::from("duplicate_query_filter")),
            ("field", JsonValue::from("query")),
            ("value", JsonValue::from(prefix.to_string())),
            ("hint", JsonValue::from(hint)),
        ]),
    )
}

fn parse_title_term(raw: &str) -> Result<String, AppError> {
    reject_unquoted_bare_operator_chars(raw, raw)?;
    let term = parse_scalar_value(raw)?;
    if term.is_empty() {
        return Err(AppError::invalid_query("title term must not be empty"));
    }
    Ok(term)
}

fn parse_negated_title_term(raw: &str) -> Result<String, AppError> {
    let term = raw
        .strip_prefix('-')
        .expect("negated term should start with '-'");
    if term.is_empty() || term.starts_with('-') {
        return Err(AppError::invalid_query("Invalid negated title term"));
    }
    if term.contains(':') && !is_quoted_scalar(term) {
        return Err(unknown_filter_prefix_error(raw));
    }
    reject_unquoted_bare_operator_chars(term, raw)?;
    parse_title_term(term)
}

fn reject_unquoted_bare_operator_chars(raw_term: &str, error_value: &str) -> Result<(), AppError> {
    if raw_term.starts_with('"') {
        return Ok(());
    }
    if raw_term.contains('|') || raw_term.contains('&') || raw_term.starts_with('!') {
        return Err(bare_operator_token_error(error_value));
    }
    Ok(())
}

fn validate_title_terms(query: &EntryQuery) -> Result<(), AppError> {
    let term_count = query.title_terms.len() + query.negated_title_terms.len();
    let term_count = term_count
        + query
            .term_groups
            .iter()
            .map(TermExpr::term_count)
            .sum::<usize>()
        + query
            .negated_term_groups
            .iter()
            .map(TermExpr::term_count)
            .sum::<usize>();
    if term_count > MAX_TITLE_TERMS {
        return Err(AppError::invalid_query(format!(
            "query must include at most {MAX_TITLE_TERMS} title terms"
        )));
    }
    let positive_terms = query
        .title_terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if query
        .negated_title_terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .any(|term| positive_terms.contains(&term))
    {
        return Err(AppError::invalid_query(
            "positive and negative title terms cannot match",
        ));
    }
    Ok(())
}

fn unknown_filter_prefix_error(token: &str) -> AppError {
    AppError::invalid_query_with_details(
        format!("Unknown query filter prefix: {token}"),
        error_details([
            ("kind", JsonValue::from("unknown_filter_prefix")),
            ("field", JsonValue::from("query")),
            ("value", JsonValue::from(token.to_string())),
            (
                "hint",
                JsonValue::from("quote_token_to_search_literal_text"),
            ),
        ]),
    )
}

fn bare_operator_token_error(token: &str) -> AppError {
    AppError::invalid_query_with_details(
        format!("Bare operator token in query: {token}"),
        error_details([
            ("kind", JsonValue::from("bare_operator_token")),
            ("field", JsonValue::from("query")),
            ("value", JsonValue::from(token.to_string())),
            (
                "hint",
                JsonValue::from("quote_token_to_search_literal_text"),
            ),
        ]),
    )
}

/// Tokenizes query text while honoring quoted and parenthesized expression segments.
///
/// Quoted segments are delimited by `"` and may contain escaped `\"` and `\\`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueryToken {
    value: String,
    span: std::ops::Range<usize>,
    whitespace_before: bool,
    expression_tokens: Vec<expr::ExprToken>,
}

impl QueryToken {
    fn expression_after_prefix(&self, prefix: &str) -> Result<Vec<expr::ExprToken>, AppError> {
        let mut tokens = self.expression_tokens.clone();
        let Some(first) = tokens.first_mut() else {
            return Ok(tokens);
        };
        let expr::ExprTokenKind::Literal { value, quoted } = &mut first.kind else {
            return Err(AppError::invalid_query("Invalid query expression"));
        };
        let Some(remainder) = value.strip_prefix(prefix) else {
            return Err(AppError::invalid_query("Invalid query expression"));
        };
        let replacement_kind = match remainder.to_ascii_uppercase().as_str() {
            "AND" => Some(expr::ExprTokenKind::And),
            "OR" => Some(expr::ExprTokenKind::Or),
            "NOT" => Some(expr::ExprTokenKind::Not),
            _ => None,
        };
        *value = remainder.to_string();
        *quoted = false;
        if value.is_empty() {
            tokens.remove(0);
            if let Some(first) = tokens.first_mut() {
                first.whitespace_before = false;
            }
        } else if let Some(kind) = replacement_kind {
            tokens[0].kind = kind;
        }
        Ok(tokens)
    }
}

fn tokenize(raw: &str) -> Result<Vec<QueryToken>, AppError> {
    let expression_tokens = expr::lex_expr(raw)?;
    let mut items = Vec::<QueryToken>::new();
    let mut depth = 0usize;
    for token in expression_tokens {
        let starts_new_item = token.whitespace_before && depth == 0;
        if starts_new_item || items.is_empty() {
            items.push(QueryToken {
                value: String::new(),
                span: token.span.clone(),
                whitespace_before: token.whitespace_before,
                expression_tokens: Vec::new(),
            });
        }
        let item = items.last_mut().expect("query item");
        item.span.end = token.span.end;
        match token.kind {
            expr::ExprTokenKind::LParen
                if depth > 0
                    || item.expression_tokens.is_empty()
                    || item.expression_tokens.last().is_some_and(|previous| {
                        matches!(
                            &previous.kind,
                            expr::ExprTokenKind::Literal { value, quoted: false }
                                if matches!(value.as_str(), "-" | "tag:" | "-tag:")
                        ) || matches!(
                            previous.kind,
                            expr::ExprTokenKind::And
                                | expr::ExprTokenKind::Or
                                | expr::ExprTokenKind::Not
                        )
                    }) =>
            {
                depth += 1;
            }
            expr::ExprTokenKind::RParen => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        item.expression_tokens.push(token);
    }
    if depth > 0 {
        return Err(AppError::invalid_query("Unclosed parenthesis in query"));
    }
    for item in &mut items {
        item.value = raw[item.span.clone()].to_string();
    }
    Ok(items)
}

fn is_bare_operator_token(token: &str) -> bool {
    !is_quoted_scalar(token) && matches!(token, "AND" | "OR" | "NOT" | "&" | "|" | "!")
}

/// Parses scalar query values with optional quote escaping.
fn parse_scalar_value(raw: &str) -> Result<String, AppError> {
    if raw.starts_with('"') {
        let chars = raw.chars().collect::<Vec<_>>();
        let (literal, consumed) = expr::read_quoted_literal(&chars)?;
        if consumed != chars.len() {
            return Err(AppError::invalid_query("Invalid quoted value"));
        }
        return Ok(literal);
    }
    if raw.contains('"') {
        return Err(AppError::invalid_query("Invalid quoted value"));
    }
    Ok(raw.to_string())
}

fn is_quoted_scalar(raw: &str) -> bool {
    raw.starts_with('"') && raw.ends_with('"')
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
    use super::{EntryQuery, FeedFilter, TagExpr, TermExpr};
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
    fn rejects_empty_quoted_scalar_values() {
        for raw in [r#"feed:"""#, r#"after:"""#] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
            assert_eq!(error.message(), "quoted literal must not be empty");
        }
    }

    #[test]
    fn rejects_unescaped_quote_inside_scalar_values() {
        let error = EntryQuery::parse(Some(r#""a"b"c""#), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        assert_eq!(error.message(), "Invalid quoted value");
    }

    #[test]
    fn rejects_unknown_escape_sequences_inside_quoted_values() {
        for (raw, value) in [(r#""a\x""#, r#"\x"#), (r#"tag:"a\x""#, r#"\x"#)] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
            let details = error.details().expect("details");
            assert_eq!(details["kind"], "invalid_escape_sequence");
            assert_eq!(details["field"], "query");
            assert_eq!(details["value"], value);
            assert_eq!(details["hint"], "escape_backslash_as_double_backslash");
        }
    }

    #[test]
    fn parses_escaped_backslash_inside_quoted_term() {
        let query = EntryQuery::parse(Some(r#""C:\\Users""#), Some("unread")).expect("query");
        assert_eq!(query.title_terms, vec!["C:\\Users".to_string()]);
    }

    #[test]
    fn rejects_title_prefix_with_quote_hint() {
        let error = EntryQuery::parse(Some("title:\"First\""), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("details");
        assert_eq!(details["kind"], "unknown_filter_prefix");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], "title:\"First\"");
        assert_eq!(details["hint"], "quote_token_to_search_literal_text");
    }

    #[test]
    fn parse_bare_quoted_and_explicit_title_terms() {
        let query = EntryQuery::parse(Some("First \"Second Entry\" \"tag:rust\""), Some("unread"))
            .expect("query");
        assert_eq!(
            query.title_terms,
            vec![
                "First".to_string(),
                "Second Entry".to_string(),
                "tag:rust".to_string()
            ]
        );
        assert!(query.negated_title_terms.is_empty());
    }

    #[test]
    fn parses_tag_filter_and_following_title_terms_separately() {
        let query = EntryQuery::parse(Some("tag:rust async"), Some("unread")).expect("query");
        assert_eq!(query.tag_expr, Some(TagExpr::Tag("rust".to_string())));
        assert_eq!(query.title_terms, vec!["async".to_string()]);

        let query = EntryQuery::parse(Some("tag:rust -nightly"), Some("unread")).expect("query");
        assert_eq!(query.tag_expr, Some(TagExpr::Tag("rust".to_string())));
        assert_eq!(query.negated_title_terms, vec!["nightly".to_string()]);

        let query = EntryQuery::parse(Some("tag:rust \"foo bar\""), Some("unread")).expect("query");
        assert_eq!(query.tag_expr, Some(TagExpr::Tag("rust".to_string())));
        assert_eq!(query.title_terms, vec!["foo bar".to_string()]);

        let query = EntryQuery::parse(Some("tag:(a) (b)"), Some("unread")).expect("query");
        assert_eq!(query.tag_expr, Some(TagExpr::Tag("a".to_string())));
        assert_eq!(query.title_terms, vec!["b".to_string()]);
    }

    #[test]
    fn rejects_unknown_filter_prefix_with_quote_hint() {
        let error = EntryQuery::parse(Some("foo:bar"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("details");
        assert_eq!(details["kind"], "unknown_filter_prefix");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], "foo:bar");
        assert_eq!(details["hint"], "quote_token_to_search_literal_text");
    }

    #[test]
    fn rejects_capitalized_filter_prefix_as_unknown() {
        let error = EntryQuery::parse(Some("Tag:rust"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("details");
        assert_eq!(details["kind"], "unknown_filter_prefix");
        assert_eq!(details["value"], "Tag:rust");

        let query = EntryQuery::parse(Some("\"Tag:rust\""), Some("unread")).expect("query");
        assert_eq!(query.title_terms, vec!["Tag:rust".to_string()]);
    }

    #[test]
    fn rejects_bare_operator_tokens_with_quote_hint() {
        for token in ["AND", "OR", "NOT", "&", "|", "!"] {
            let error = EntryQuery::parse(Some(token), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
            let details = error.details().expect("details");
            assert_eq!(details["kind"], "bare_operator_token");
            assert_eq!(details["field"], "query");
            assert_eq!(details["value"], token);
            assert_eq!(details["hint"], "quote_token_to_search_literal_text");
        }

        let error = EntryQuery::parse(Some("tag:A | B"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("details");
        assert_eq!(details["kind"], "bare_operator_token");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], "|");
        assert_eq!(details["hint"], "quote_token_to_search_literal_text");
    }

    #[test]
    fn parses_quoted_operator_token_as_title_term() {
        let query = EntryQuery::parse(Some("\"OR\""), Some("unread")).expect("query");
        assert_eq!(query.title_terms, vec!["OR".to_string()]);
    }

    #[test]
    fn parses_parenthesized_expressions_across_whitespace() {
        let query = EntryQuery::parse(
            Some(r#"tag:( A | B ) -tag:( C | D ) ( rust cli | "machine learning" ) -( sponsored | 広告 )"#),
            Some("unread"),
        )
        .expect("query");

        assert_eq!(
            query.tag_expr,
            Some(TagExpr::And(vec![
                TagExpr::Not(Box::new(TagExpr::Or(vec![
                    TagExpr::Tag("C".to_string()),
                    TagExpr::Tag("D".to_string()),
                ]))),
                TagExpr::Or(vec![
                    TagExpr::Tag("A".to_string()),
                    TagExpr::Tag("B".to_string()),
                ]),
            ]))
        );
        assert_eq!(
            query.term_groups,
            vec![TermExpr::Or(vec![
                TermExpr::And(vec![
                    TermExpr::Term("cli".to_string()),
                    TermExpr::Term("rust".to_string()),
                ]),
                TermExpr::Term("machine learning".to_string()),
            ])]
        );
        assert_eq!(
            query.negated_term_groups,
            vec![TermExpr::Or(vec![
                TermExpr::Term("sponsored".to_string()),
                TermExpr::Term("広告".to_string()),
            ])]
        );
    }

    #[test]
    fn parse_negated_title_terms() {
        let query = EntryQuery::parse(
            Some("rust -nightly -\"sponsored post\" \"-rc1\""),
            Some("unread"),
        )
        .expect("query");
        assert_eq!(
            query.title_terms,
            vec!["rust".to_string(), "-rc1".to_string()]
        );
        assert_eq!(
            query.negated_title_terms,
            vec!["nightly".to_string(), "sponsored post".to_string()]
        );
    }

    #[test]
    fn parse_title_term_groups() {
        let query = EntryQuery::parse(
            Some("(alpha|アルファ) (beta|ベータ) -(gamma|ガンマ)"),
            Some("unread"),
        )
        .expect("query");
        assert_eq!(
            query.term_groups,
            vec![
                TermExpr::Or(vec![
                    TermExpr::Term("alpha".to_string()),
                    TermExpr::Term("アルファ".to_string()),
                ]),
                TermExpr::Or(vec![
                    TermExpr::Term("beta".to_string()),
                    TermExpr::Term("ベータ".to_string()),
                ]),
            ]
        );
        assert_eq!(
            query.negated_term_groups,
            vec![TermExpr::Or(vec![
                TermExpr::Term("gamma".to_string()),
                TermExpr::Term("ガンマ".to_string()),
            ])]
        );
    }

    #[test]
    fn rejects_collapsed_title_term_group_contradictions() {
        for raw in ["(foo) -foo", "foo -(foo)"] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
            assert_eq!(
                error.message(),
                "positive and negative title terms cannot match"
            );
        }
    }

    #[test]
    fn hoists_collapsed_positive_title_term_group() {
        let query = EntryQuery::parse(Some("(foo)"), Some("unread")).expect("query");
        assert_eq!(query.title_terms, vec!["foo".to_string()]);
        assert!(query.term_groups.is_empty());
    }

    #[test]
    fn hoists_collapsed_negated_title_term_group() {
        let query = EntryQuery::parse(Some("-(foo)"), Some("unread")).expect("query");
        assert_eq!(query.negated_title_terms, vec!["foo".to_string()]);
        assert!(query.negated_term_groups.is_empty());
    }

    #[test]
    fn keeps_multi_term_title_groups_grouped() {
        let query = EntryQuery::parse(Some("(a|b)"), Some("unread")).expect("query");
        assert!(query.title_terms.is_empty());
        assert_eq!(
            query.term_groups,
            vec![TermExpr::Or(vec![
                TermExpr::Term("a".to_string()),
                TermExpr::Term("b".to_string()),
            ])]
        );
    }

    #[test]
    fn parse_nested_title_term_group() {
        let query =
            EntryQuery::parse(Some("((echo&delta)|\"共同声明\")"), Some("unread")).expect("query");
        assert_eq!(
            query.term_groups,
            vec![TermExpr::Or(vec![
                TermExpr::And(vec![
                    TermExpr::Term("delta".to_string()),
                    TermExpr::Term("echo".to_string()),
                ]),
                TermExpr::Term("共同声明".to_string()),
            ])]
        );
    }

    #[test]
    fn rejects_adjacent_term_primary_without_separator() {
        let error = EntryQuery::parse(Some("(a|b)x"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_adjacent_tag_primary_without_separator() {
        for raw in ["tag:(a)(b)", "tag:(a)( b )"] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
        }
    }

    #[test]
    fn parses_explicit_and_before_whitespace_padded_tag_group() {
        let query = EntryQuery::parse(Some("tag:(a)&( b )"), Some("unread")).expect("query");
        assert_eq!(
            query.tag_expr,
            Some(TagExpr::And(vec![
                TagExpr::Tag("a".to_string()),
                TagExpr::Tag("b".to_string()),
            ]))
        );
    }

    #[test]
    fn parses_explicit_and_before_whitespace_padded_groups() {
        let query = EntryQuery::parse(Some("(a)&( b )"), Some("unread")).expect("query");
        assert_eq!(
            query.term_groups,
            vec![TermExpr::And(vec![
                TermExpr::Term("a".to_string()),
                TermExpr::Term("b".to_string()),
            ])]
        );

        let query = EntryQuery::parse(Some("tag:a&( b )"), Some("unread")).expect("query");
        assert_eq!(
            query.tag_expr,
            Some(TagExpr::And(vec![
                TagExpr::Tag("a".to_string()),
                TagExpr::Tag("b".to_string()),
            ]))
        );
    }

    #[test]
    fn rejects_operator_characters_inside_unquoted_bare_terms() {
        for raw in ["a|b", "a&b", "!foo", "-a|b", "-a&b", "-!foo"] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
            let details = error.details().expect("details");
            assert_eq!(details["kind"], "bare_operator_token");
            assert_eq!(details["field"], "query");
            assert_eq!(details["value"], raw);
            assert_eq!(details["hint"], "quote_token_to_search_literal_text");
        }
    }

    #[test]
    fn parses_non_operator_punctuation_inside_unquoted_bare_terms() {
        let query =
            EntryQuery::parse(Some("Rust(2024) release-notes"), Some("unread")).expect("query");
        assert_eq!(
            query.title_terms,
            vec!["Rust(2024)".to_string(), "release-notes".to_string()]
        );
        assert!(query.term_groups.is_empty());
    }

    #[test]
    fn rejects_not_inside_negated_title_term_group() {
        for raw in ["-(!A)", "-(NOT A)"] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
        }
    }

    #[test]
    fn rejects_unknown_prefix_inside_title_term_group() {
        let error = EntryQuery::parse(Some("(foo:bar|baz)"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("details");
        assert_eq!(details["kind"], "unknown_filter_prefix");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], "foo:bar");
        assert_eq!(details["hint"], "quote_token_to_search_literal_text");
    }

    #[test]
    fn counts_title_term_group_members_toward_limit() {
        let raw = format!(
            "({})",
            (0..33)
                .map(|i| format!("t{i}"))
                .collect::<Vec<_>>()
                .join("|")
        );
        let error = EntryQuery::parse(Some(&raw), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_invalid_negated_title_terms() {
        for raw in ["-", "--foo"] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
        }
    }

    #[test]
    fn rejects_negated_unknown_filter_prefix_with_quote_hint() {
        let error = EntryQuery::parse(Some("-title:x"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("details");
        assert_eq!(details["kind"], "unknown_filter_prefix");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], "-title:x");
        assert_eq!(details["hint"], "quote_token_to_search_literal_text");
    }

    #[test]
    fn rejects_empty_title_terms() {
        for raw in [r#""""#, r#"-"""#] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
        }
    }

    #[test]
    fn rejects_empty_quoted_literals_inside_query_expressions() {
        for raw in [r#"tag:"""#, r#"(""|foo)"#] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
        }
    }

    #[test]
    fn rejects_title_term_count_over_limit() {
        let raw = (0..33)
            .map(|i| format!("t{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let error = EntryQuery::parse(Some(&raw), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_direct_title_term_contradiction() {
        let error = EntryQuery::parse(Some("Foo -foo"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_duplicate_feed_tokens() {
        let error = EntryQuery::parse(Some("feed:1 feed:2"), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("details");
        assert_eq!(details["kind"], "duplicate_query_filter");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], "feed:");
        assert_eq!(details["hint"], "remove_duplicate_filter");
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
        let details = error.details().expect("details");
        assert_eq!(details["kind"], "duplicate_query_filter");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], "after:");
        assert_eq!(details["hint"], "remove_duplicate_filter");
    }

    #[test]
    fn rejects_duplicate_before_tokens() {
        let error = EntryQuery::parse(Some("before:2026-01-01 before:2026-01-02"), Some("unread"))
            .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("details");
        assert_eq!(details["kind"], "duplicate_query_filter");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], "before:");
        assert_eq!(details["hint"], "remove_duplicate_filter");
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
    fn parses_unknown_tokens_as_title_terms() {
        let query = EntryQuery::parse(Some("oops"), Some("unread")).expect("query");
        assert_eq!(query.title_terms, vec!["oops".to_string()]);
    }

    #[test]
    fn unread_keyword_is_noop_when_alias_is_unavailable() {
        let query = EntryQuery::parse(Some("unread"), None).expect("query");
        assert!(query.tag_expr.is_none());
    }
}
