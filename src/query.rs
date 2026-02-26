//! Query parser for entry filters.

use crate::error::AppError;
use crate::time::current_epoch;
use ::time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};
use std::collections::HashSet;

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

const MAX_TAG_TOKENS: usize = 64;
const MAX_TAG_AST_DEPTH: usize = 16;

impl TagExpr {
    /// Returns a stable canonical representation used for hash validation.
    pub(crate) fn canonical(&self) -> String {
        match self {
            TagExpr::Tag(tag) => format!("tag:{}", escape_tag_literal(tag)),
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
    pub fn parse(raw: Option<&str>, unread_tag: &str) -> Result<Self, AppError> {
        let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        Self::parse_with_now(raw, unread_tag, current_epoch(), local_offset)
    }

    fn parse_with_now(
        raw: Option<&str>,
        unread_tag: &str,
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
                tag_terms.push(TagExpr::Tag(unread_tag.to_string()));
                index += 1;
                continue;
            }
            if let Some(value) = token.strip_prefix("tag:") {
                let mut parts = Vec::new();
                if !value.is_empty() {
                    parts.push(value.to_string());
                }
                index += 1;
                while index < tokens.len() && !is_top_level_token(&tokens[index]) {
                    parts.push(tokens[index].clone());
                    index += 1;
                }
                if parts.is_empty() {
                    return Err(AppError::invalid_query("tag: requires a value"));
                }
                let expr = parse_tag_expr(&parts.join(" "))?;
                tag_terms.push(expr);
                continue;
            }
            if let Some(value) = token.strip_prefix("-tag:") {
                let mut parts = Vec::new();
                if !value.is_empty() {
                    parts.push(value.to_string());
                }
                index += 1;
                while index < tokens.len() && !is_top_level_token(&tokens[index]) {
                    parts.push(tokens[index].clone());
                    index += 1;
                }
                if parts.is_empty() {
                    return Err(AppError::invalid_query("-tag: requires a value"));
                }
                let inner = parse_minus_tag_expr(&parts.join(" "))?;
                tag_terms.push(TagExpr::Not(Box::new(inner)));
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
                index += 1;
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
                query.title = Some(parse_scalar_value(value)?);
                index += 1;
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
                query.after = Some(parse_date_or_relative_to_epoch(
                    &parse_scalar_value(value)?,
                    now_epoch_utc,
                    local_offset,
                )?);
                index += 1;
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
                query.before = Some(parse_date_or_relative_to_epoch(
                    &parse_scalar_value(value)?,
                    now_epoch_utc,
                    local_offset,
                )?);
                index += 1;
                continue;
            }
            return Err(AppError::invalid_query(format!(
                "Unknown query token: {token}"
            )));
        }

        if !tag_terms.is_empty() {
            let expr = normalize_tag_expr(and_all(tag_terms));
            if has_direct_tag_conflict(&expr) {
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

/// Parses an ISO date (YYYY-MM-DD) to epoch seconds at local midnight.
fn parse_date_to_epoch(value: &str, local_offset: UtcOffset) -> Result<i64, AppError> {
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
    Ok(datetime.assume_offset(local_offset).unix_timestamp())
}

/// Parses either absolute date (`YYYY-MM-DD`) or relative duration (`N[d|w|m|y]`) to epoch seconds.
fn parse_date_or_relative_to_epoch(
    value: &str,
    now_epoch_utc: i64,
    local_offset: UtcOffset,
) -> Result<i64, AppError> {
    if let Ok(epoch) = parse_date_to_epoch(value, local_offset) {
        return Ok(epoch);
    }
    parse_relative_date_to_epoch(value, now_epoch_utc, local_offset)
}

/// Parses relative date duration (`N[d|w|m|y]`) anchored at local-date midnight.
fn parse_relative_date_to_epoch(
    value: &str,
    now_epoch_utc: i64,
    local_offset: UtcOffset,
) -> Result<i64, AppError> {
    let (amount, unit) = parse_relative_duration(value)?;
    let now_utc = OffsetDateTime::from_unix_timestamp(now_epoch_utc)
        .map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let base_date = now_utc.to_offset(local_offset).date();
    let target_date = match unit {
        'd' => base_date
            .checked_sub(time::Duration::days(amount as i64))
            .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?,
        'w' => base_date
            .checked_sub(time::Duration::days((amount as i64) * 7))
            .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?,
        'm' => subtract_months_clamped(base_date, amount)?,
        'y' => subtract_years_clamped(base_date, amount)?,
        _ => return Err(AppError::invalid_query("Invalid relative date")),
    };
    Ok(PrimitiveDateTime::new(target_date, Time::MIDNIGHT)
        .assume_offset(local_offset)
        .unix_timestamp())
}

/// Parses `N[d|w|m|y]` into (amount, unit).
fn parse_relative_duration(value: &str) -> Result<(u32, char), AppError> {
    if value.len() < 2 {
        return Err(AppError::invalid_query("Invalid relative date"));
    }
    let (number, unit) = value.split_at(value.len() - 1);
    if number.is_empty() || number.starts_with('-') || !number.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err(AppError::invalid_query("Invalid relative date"));
    }
    let amount = number
        .parse::<u32>()
        .map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let unit = unit
        .chars()
        .next()
        .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?;
    if !matches!(unit, 'd' | 'w' | 'm' | 'y') {
        return Err(AppError::invalid_query("Invalid relative date"));
    }
    Ok((amount, unit))
}

/// Subtracts months while clamping day at month-end.
fn subtract_months_clamped(date: Date, months: u32) -> Result<Date, AppError> {
    let months_i32 =
        i32::try_from(months).map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let month_index = i32::from(u8::from(date.month())) - 1;
    let total = date
        .year()
        .checked_mul(12)
        .and_then(|value| value.checked_add(month_index))
        .and_then(|value| value.checked_sub(months_i32))
        .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?;
    let year = total.div_euclid(12);
    let month = Month::try_from((total.rem_euclid(12) + 1) as u8)
        .map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let day = date.day().min(days_in_month(year, month));
    Date::from_calendar_date(year, month, day)
        .map_err(|_| AppError::invalid_query("Invalid relative date"))
}

/// Subtracts years while clamping day at month-end.
fn subtract_years_clamped(date: Date, years: u32) -> Result<Date, AppError> {
    let years_i32 =
        i32::try_from(years).map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let year = date
        .year()
        .checked_sub(years_i32)
        .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?;
    let month = date.month();
    let day = date.day().min(days_in_month(year, month));
    Date::from_calendar_date(year, month, day)
        .map_err(|_| AppError::invalid_query("Invalid relative date"))
}

/// Returns the number of days in the given year/month.
fn days_in_month(year: i32, month: Month) -> u8 {
    match month {
        Month::January
        | Month::March
        | Month::May
        | Month::July
        | Month::August
        | Month::October
        | Month::December => 31,
        Month::April | Month::June | Month::September | Month::November => 30,
        Month::February => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
    }
}

/// Gregorian leap year.
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Parses a tag expression.
fn parse_tag_expr(raw: &str) -> Result<TagExpr, AppError> {
    let tokens = lex_tag_expr(raw)?;
    if tokens.is_empty() {
        return Err(AppError::invalid_query("tag: requires a value"));
    }
    let mut parser = TagExprParser::new(tokens);
    let expr = parser.parse_or()?;
    if parser.peek().is_some() {
        return Err(AppError::invalid_query("Invalid tag expression"));
    }
    let normalized = normalize_tag_expr(expr);
    validate_tag_expr_limits(&normalized)?;
    Ok(normalized)
}

/// Parses `-tag:` value and rejects nested NOT expressions.
fn parse_minus_tag_expr(raw: &str) -> Result<TagExpr, AppError> {
    let expr = parse_tag_expr(raw)?;
    if expr.contains_not() {
        return Err(AppError::invalid_query(
            "-tag: expression must not include NOT/!",
        ));
    }
    Ok(expr)
}

/// Validates expression-level guardrails for parser safety.
fn validate_tag_expr_limits(expr: &TagExpr) -> Result<(), AppError> {
    if expr.tag_token_count() > MAX_TAG_TOKENS {
        return Err(AppError::invalid_query(
            "Tag expression exceeds max tag tokens",
        ));
    }
    if expr.max_depth() > MAX_TAG_AST_DEPTH {
        return Err(AppError::invalid_query("Tag expression exceeds max depth"));
    }
    Ok(())
}

/// Escapes a tag literal for canonical serialization.
fn escape_tag_literal(value: &str) -> String {
    value.replace('\\', "\\\\").replace(':', "\\:")
}

/// Builds an AND expression from a list of terms.
fn and_all(mut terms: Vec<TagExpr>) -> TagExpr {
    if terms.len() == 1 {
        return terms.remove(0);
    }
    TagExpr::And(terms)
}

/// Normalizes tag expressions for deterministic hashing and SQL generation.
fn normalize_tag_expr(expr: TagExpr) -> TagExpr {
    match expr {
        TagExpr::Tag(_) => expr,
        TagExpr::Not(inner) => TagExpr::Not(Box::new(normalize_tag_expr(*inner))),
        TagExpr::And(items) => normalize_variadic(items, true),
        TagExpr::Or(items) => normalize_variadic(items, false),
    }
}

/// Normalizes variadic operators by flattening, deduplicating, and sorting.
fn normalize_variadic(items: Vec<TagExpr>, is_and: bool) -> TagExpr {
    let mut flat = Vec::new();
    for item in items.into_iter().map(normalize_tag_expr) {
        match item {
            TagExpr::And(children) if is_and => flat.extend(children),
            TagExpr::Or(children) if !is_and => flat.extend(children),
            other => flat.push(other),
        }
    }

    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for item in flat {
        let canonical = item.canonical();
        if seen.insert(canonical) {
            unique.push(item);
        }
    }
    unique.sort_by_key(TagExpr::canonical);
    if unique.len() == 1 {
        return unique.remove(0);
    }
    if is_and {
        TagExpr::And(unique)
    } else {
        TagExpr::Or(unique)
    }
}

/// Detects direct contradictions in top-level AND terms.
fn has_direct_tag_conflict(expr: &TagExpr) -> bool {
    let TagExpr::And(items) = expr else {
        return false;
    };
    let mut include = HashSet::new();
    let mut exclude = HashSet::new();
    for item in items {
        match item {
            TagExpr::Tag(tag) => {
                include.insert(tag.clone());
            }
            TagExpr::Not(inner) => {
                if let TagExpr::Tag(tag) = inner.as_ref() {
                    exclude.insert(tag.clone());
                }
            }
            _ => {}
        }
    }
    include.iter().any(|tag| exclude.contains(tag))
}

/// Token used by tag expression parser.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TagToken {
    /// Literal tag name.
    Literal(String),
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `AND` or `&`
    And,
    /// `OR` or `|`
    Or,
    /// `NOT` or `!`
    Not,
}

/// Lexes a tag expression into tokens.
fn lex_tag_expr(raw: &str) -> Result<Vec<TagToken>, AppError> {
    let chars = raw.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        match ch {
            '(' => {
                tokens.push(TagToken::LParen);
                index += 1;
            }
            ')' => {
                tokens.push(TagToken::RParen);
                index += 1;
            }
            '&' => {
                tokens.push(TagToken::And);
                index += 1;
            }
            '|' => {
                tokens.push(TagToken::Or);
                index += 1;
            }
            '!' => {
                tokens.push(TagToken::Not);
                index += 1;
            }
            '"' => {
                let (literal, consumed) = read_quoted_literal(&chars[index..])?;
                tokens.push(TagToken::Literal(literal));
                index += consumed;
            }
            _ => {
                let start = index;
                while index < chars.len() {
                    let current = chars[index];
                    if current.is_whitespace() || matches!(current, '(' | ')' | '&' | '|' | '!') {
                        break;
                    }
                    index += 1;
                }
                if start == index {
                    return Err(AppError::invalid_query("Invalid tag expression"));
                }
                let literal = chars[start..index].iter().collect::<String>();
                match literal.to_ascii_uppercase().as_str() {
                    "AND" => tokens.push(TagToken::And),
                    "OR" => tokens.push(TagToken::Or),
                    "NOT" => tokens.push(TagToken::Not),
                    _ => tokens.push(TagToken::Literal(literal)),
                }
            }
        }
    }
    Ok(tokens)
}

/// Reads a quoted literal from a char slice.
fn read_quoted_literal(chars: &[char]) -> Result<(String, usize), AppError> {
    if chars.first() != Some(&'"') {
        return Err(AppError::invalid_query("Invalid quoted tag literal"));
    }
    let mut index = 1usize;
    let mut out = String::new();
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            return Ok((out, index + 1));
        }
        if ch == '\\' {
            index += 1;
            if index >= chars.len() {
                return Err(AppError::invalid_query("Invalid escape sequence"));
            }
            match chars[index] {
                '\\' | '"' => out.push(chars[index]),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            index += 1;
            continue;
        }
        out.push(ch);
        index += 1;
    }
    Err(AppError::invalid_query("Unclosed quote in query"))
}

/// Tag expression parser.
struct TagExprParser {
    tokens: Vec<TagToken>,
    index: usize,
}

impl TagExprParser {
    /// Creates a parser instance.
    fn new(tokens: Vec<TagToken>) -> Self {
        Self { tokens, index: 0 }
    }

    /// Returns current token.
    fn peek(&self) -> Option<&TagToken> {
        self.tokens.get(self.index)
    }

    /// Consumes and returns current token.
    fn next(&mut self) -> Option<TagToken> {
        let token = self.tokens.get(self.index).cloned();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    /// Parses OR-level expression.
    fn parse_or(&mut self) -> Result<TagExpr, AppError> {
        let mut terms = vec![self.parse_and()?];
        while matches!(self.peek(), Some(TagToken::Or)) {
            self.next();
            terms.push(self.parse_and()?);
        }
        if terms.len() == 1 {
            Ok(terms.remove(0))
        } else {
            Ok(TagExpr::Or(terms))
        }
    }

    /// Parses AND-level expression with implicit AND.
    fn parse_and(&mut self) -> Result<TagExpr, AppError> {
        let mut terms = vec![self.parse_unary()?];
        loop {
            if matches!(self.peek(), Some(TagToken::And)) {
                self.next();
                terms.push(self.parse_unary()?);
                continue;
            }
            if matches!(
                self.peek(),
                Some(TagToken::Literal(_)) | Some(TagToken::LParen) | Some(TagToken::Not)
            ) {
                terms.push(self.parse_unary()?);
                continue;
            }
            break;
        }
        if terms.len() == 1 {
            Ok(terms.remove(0))
        } else {
            Ok(TagExpr::And(terms))
        }
    }

    /// Parses unary `NOT` expression.
    fn parse_unary(&mut self) -> Result<TagExpr, AppError> {
        if matches!(self.peek(), Some(TagToken::Not)) {
            self.next();
            return Ok(TagExpr::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    /// Parses primary expression.
    fn parse_primary(&mut self) -> Result<TagExpr, AppError> {
        match self.next() {
            Some(TagToken::Literal(value)) => Ok(TagExpr::Tag(value)),
            Some(TagToken::LParen) => {
                let expr = self.parse_or()?;
                match self.next() {
                    Some(TagToken::RParen) => Ok(expr),
                    _ => Err(AppError::invalid_query(
                        "Unclosed parenthesis in tag expression",
                    )),
                }
            }
            _ => Err(AppError::invalid_query("Invalid tag expression")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EntryQuery, FeedFilter, TagExpr, normalize_tag_expr};
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
    fn parse_tag_filters() {
        let query = EntryQuery::parse(Some("unread tag:tech -tag:misc"), "unread").expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Tag("unread".to_string()),
            TagExpr::Tag("tech".to_string()),
            TagExpr::Not(Box::new(TagExpr::Tag("misc".to_string()))),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_feed_id_query() {
        let query = EntryQuery::parse(Some("feed:123"), "unread").expect("query");
        assert_eq!(query.feed, Some(FeedFilter::Id("123".to_string())));
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
    fn parse_relative_date_bounds() {
        let query = EntryQuery::parse_with_now(
            Some("after:1m before:3d"),
            "unread",
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
            "unread",
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
            "unread",
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
            "unread",
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
        let after =
            EntryQuery::parse_with_now(Some("after:0d"), "unread", fixed_now_utc(), fixed_jst())
                .expect("query");
        let before =
            EntryQuery::parse_with_now(Some("before:0w"), "unread", fixed_now_utc(), fixed_jst())
                .expect("query");
        let anchor = local_midnight_epoch(2026, Month::February, 26, fixed_jst());
        assert_eq!(after.after, Some(anchor));
        assert_eq!(before.before, Some(anchor));
    }

    #[test]
    fn rejects_invalid_relative_date_unit() {
        let error =
            EntryQuery::parse_with_now(Some("after:3x"), "unread", fixed_now_utc(), fixed_jst())
                .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_overflow_relative_duration() {
        let error = EntryQuery::parse_with_now(
            Some("after:2147483648y"),
            "unread",
            fixed_now_utc(),
            fixed_jst(),
        )
        .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
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
    fn deduplicates_repeated_tag_tokens() {
        let query = EntryQuery::parse(Some("tag:rust tag:rust -tag:misc -tag:misc"), "unread")
            .expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Tag("rust".to_string()),
            TagExpr::Not(Box::new(TagExpr::Tag("misc".to_string()))),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn rejects_conflicting_tag_filters() {
        let error = EntryQuery::parse(Some("tag:rust -tag:rust"), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_invalid_date_range() {
        let error =
            EntryQuery::parse(Some("after:2026-01-02 before:2026-01-02"), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_invalid_relative_date_range() {
        let error = EntryQuery::parse_with_now(
            Some("after:0d before:1y"),
            "unread",
            fixed_now_utc(),
            fixed_jst(),
        )
        .unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn parse_relative_year_from_leap_day_clamps_to_feb_28() {
        let leap_day_now = OffsetDateTime::new_utc(
            Date::from_calendar_date(2024, Month::February, 29).expect("date"),
            Time::from_hms(3, 0, 0).expect("time"),
        )
        .unix_timestamp();
        let query =
            EntryQuery::parse_with_now(Some("after:1y"), "unread", leap_day_now, fixed_jst())
                .expect("query");
        assert_eq!(
            query.after,
            Some(local_midnight_epoch(2023, Month::February, 28, fixed_jst()))
        );
    }

    #[test]
    fn parse_tag_alias_expression() {
        let query = EntryQuery::parse(Some("tag:A|B|C"), "unread").expect("query");
        let expected = normalize_tag_expr(TagExpr::Or(vec![
            TagExpr::Tag("A".to_string()),
            TagExpr::Tag("B".to_string()),
            TagExpr::Tag("C".to_string()),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_tag_parentheses_expression() {
        let query = EntryQuery::parse(Some("tag:A&(B|C)"), "unread").expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Tag("A".to_string()),
            TagExpr::Or(vec![
                TagExpr::Tag("B".to_string()),
                TagExpr::Tag("C".to_string()),
            ]),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_tag_precedence_expression() {
        let query = EntryQuery::parse(Some("tag:!A|B&C"), "unread").expect("query");
        let expected = normalize_tag_expr(TagExpr::Or(vec![
            TagExpr::Not(Box::new(TagExpr::Tag("A".to_string()))),
            TagExpr::And(vec![
                TagExpr::Tag("B".to_string()),
                TagExpr::Tag("C".to_string()),
            ]),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_tag_implicit_and_expression() {
        let query = EntryQuery::parse(Some("tag:A B"), "unread").expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Tag("A".to_string()),
            TagExpr::Tag("B".to_string()),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_tag_keywords_case_insensitive() {
        let query = EntryQuery::parse(Some("tag:a and b Or not c"), "unread").expect("query");
        let expected = normalize_tag_expr(TagExpr::Or(vec![
            TagExpr::And(vec![
                TagExpr::Tag("a".to_string()),
                TagExpr::Tag("b".to_string()),
            ]),
            TagExpr::Not(Box::new(TagExpr::Tag("c".to_string()))),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn absorbs_legacy_minus_tag_into_not() {
        let query = EntryQuery::parse(Some("-tag:C tag:A|B"), "unread").expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Not(Box::new(TagExpr::Tag("C".to_string()))),
            TagExpr::Or(vec![
                TagExpr::Tag("A".to_string()),
                TagExpr::Tag("B".to_string()),
            ]),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_minus_tag_expression_alias() {
        let query = EntryQuery::parse(Some("tag:A&B&C -tag:D|E"), "unread").expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Tag("A".to_string()),
            TagExpr::Tag("B".to_string()),
            TagExpr::Tag("C".to_string()),
            TagExpr::Not(Box::new(TagExpr::Or(vec![
                TagExpr::Tag("D".to_string()),
                TagExpr::Tag("E".to_string()),
            ]))),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn rejects_minus_tag_not_notation() {
        for raw in ["-tag:!A", "-tag:NOT A"] {
            let error = EntryQuery::parse(Some(raw), "unread").unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
        }
    }

    #[test]
    fn rejects_invalid_tag_expression() {
        for raw in ["tag:(A|)", "tag:(A B", "tag:"] {
            let error = EntryQuery::parse(Some(raw), "unread").unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
        }
    }

    #[test]
    fn rejects_unknown_tokens() {
        let error = EntryQuery::parse(Some("oops"), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_tag_expression_over_max_tokens() {
        let values = (0..65).map(|i| format!("t{i}")).collect::<Vec<_>>();
        let raw = format!("tag:{}", values.join("|"));
        let error = EntryQuery::parse(Some(&raw), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_tag_expression_over_max_depth() {
        let raw = format!("tag:{}A", "!".repeat(16));
        let error = EntryQuery::parse(Some(&raw), "unread").unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }
}
