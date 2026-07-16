use super::{TagExpr, TermExpr};
use crate::error::{AppError, error_details};
use crate::tag::{invalid_tag_name_error, validate_tag_name};
use serde_json::Value as JsonValue;
use std::collections::HashSet;

const MAX_TAG_TOKENS: usize = 64;
const MAX_TAG_AST_DEPTH: usize = 16;
const TAG_DEPTH_ERROR: &str = "Tag expression exceeds max depth";
const TERM_DEPTH_ERROR: &str = "Term expression exceeds max depth";

/// Parses a tag expression.
pub(super) fn parse_tag_expr(raw: &str) -> Result<TagExpr, AppError> {
    let tokens = lex_tag_expr(raw)?;
    if tokens.is_empty() {
        return Err(AppError::invalid_query("tag: requires a value"));
    }
    let mut parser = TagExprParser::new(tokens);
    let expr = parser.parse_or(1)?;
    if parser.peek().is_some() {
        return Err(AppError::invalid_query("Invalid tag expression"));
    }
    let normalized = normalize_tag_expr(expr);
    validate_tag_expr_limits(&normalized)?;
    Ok(normalized)
}

/// Parses `-tag:` value and rejects nested NOT expressions.
pub(super) fn parse_minus_tag_expr(raw: &str) -> Result<TagExpr, AppError> {
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
    validate_expr_depth(expr.max_depth(), TAG_DEPTH_ERROR)
}

fn validate_expr_depth(depth: usize, message: &'static str) -> Result<(), AppError> {
    if depth > MAX_TAG_AST_DEPTH {
        return Err(AppError::invalid_query(message));
    }
    Ok(())
}

/// Escapes a tag literal for canonical serialization.
pub(super) fn escape_tag_literal(value: &str) -> String {
    value.replace('\\', "\\\\").replace(':', "\\:")
}

/// Builds an AND expression from a list of terms.
pub(super) fn and_all(mut terms: Vec<TagExpr>) -> TagExpr {
    if terms.len() == 1 {
        return terms.remove(0);
    }
    TagExpr::And(terms)
}

/// Normalizes tag expressions for deterministic hashing and SQL generation.
pub(super) fn normalize_tag_expr(expr: TagExpr) -> TagExpr {
    normalize_expr(expr)
}

enum ExprParts<T> {
    Leaf(T),
    Not(Box<T>),
    And(Vec<T>),
    Or(Vec<T>),
}

trait BooleanExpr: Sized {
    fn canonical(&self) -> String;
    fn split(self) -> ExprParts<Self>;
    fn not(inner: Self) -> Self;
    fn and(items: Vec<Self>) -> Self;
    fn or(items: Vec<Self>) -> Self;
}

impl BooleanExpr for TagExpr {
    fn canonical(&self) -> String {
        TagExpr::canonical(self)
    }

    fn split(self) -> ExprParts<Self> {
        match self {
            TagExpr::Tag(tag) => ExprParts::Leaf(TagExpr::Tag(tag)),
            TagExpr::Not(inner) => ExprParts::Not(inner),
            TagExpr::And(items) => ExprParts::And(items),
            TagExpr::Or(items) => ExprParts::Or(items),
        }
    }

    fn not(inner: Self) -> Self {
        TagExpr::Not(Box::new(inner))
    }

    fn and(items: Vec<Self>) -> Self {
        TagExpr::And(items)
    }

    fn or(items: Vec<Self>) -> Self {
        TagExpr::Or(items)
    }
}

impl BooleanExpr for TermExpr {
    fn canonical(&self) -> String {
        TermExpr::canonical(self)
    }

    fn split(self) -> ExprParts<Self> {
        match self {
            TermExpr::Term(term) => ExprParts::Leaf(TermExpr::Term(term)),
            TermExpr::Not(inner) => ExprParts::Not(inner),
            TermExpr::And(items) => ExprParts::And(items),
            TermExpr::Or(items) => ExprParts::Or(items),
        }
    }

    fn not(inner: Self) -> Self {
        TermExpr::Not(Box::new(inner))
    }

    fn and(items: Vec<Self>) -> Self {
        TermExpr::And(items)
    }

    fn or(items: Vec<Self>) -> Self {
        TermExpr::Or(items)
    }
}

fn normalize_expr<T: BooleanExpr>(expr: T) -> T {
    match expr.split() {
        ExprParts::Leaf(expr) => expr,
        ExprParts::Not(inner) => T::not(normalize_expr(*inner)),
        ExprParts::And(items) => normalize_variadic(items, true),
        ExprParts::Or(items) => normalize_variadic(items, false),
    }
}

/// Normalizes variadic operators by flattening, deduplicating, and sorting.
fn normalize_variadic<T: BooleanExpr>(items: Vec<T>, is_and: bool) -> T {
    let mut flat = Vec::new();
    for item in items.into_iter().map(normalize_expr) {
        match item.split() {
            ExprParts::And(children) if is_and => flat.extend(children),
            ExprParts::Or(children) if !is_and => flat.extend(children),
            ExprParts::Leaf(expr) => flat.push(expr),
            ExprParts::Not(inner) => flat.push(T::not(*inner)),
            ExprParts::And(children) => flat.push(T::and(children)),
            ExprParts::Or(children) => flat.push(T::or(children)),
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
    // TODO: If parser constraints ever change, reject or model empty variadic expressions explicitly.
    unique.sort_by_key(BooleanExpr::canonical);
    if unique.len() == 1 {
        return unique.remove(0);
    }
    if is_and {
        T::and(unique)
    } else {
        T::or(unique)
    }
}

/// Detects direct contradictions in top-level AND terms.
pub(super) fn has_direct_tag_conflict(expr: &TagExpr) -> bool {
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
    Literal { value: String, quoted: bool },
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
                tokens.push(TagToken::Literal {
                    value: literal,
                    quoted: true,
                });
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
                    _ => tokens.push(TagToken::Literal {
                        value: literal,
                        quoted: false,
                    }),
                }
            }
        }
    }
    Ok(tokens)
}

/// Reads a quoted literal from a char slice.
pub(super) fn read_quoted_literal(chars: &[char]) -> Result<(String, usize), AppError> {
    if chars.first() != Some(&'"') {
        return Err(AppError::invalid_query("Invalid quoted tag literal"));
    }
    let mut index = 1usize;
    let mut out = String::new();
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            if out.is_empty() {
                return Err(AppError::invalid_query("quoted literal must not be empty"));
            }
            return Ok((out, index + 1));
        }
        if ch == '\\' {
            index += 1;
            if index >= chars.len() {
                return Err(invalid_escape_sequence_error("\\"));
            }
            match chars[index] {
                '\\' | '"' => out.push(chars[index]),
                other => return Err(invalid_escape_sequence_error(format!("\\{other}"))),
            }
            index += 1;
            continue;
        }
        out.push(ch);
        index += 1;
    }
    Err(AppError::invalid_query("Unclosed quote in query"))
}

fn invalid_escape_sequence_error(value: impl Into<String>) -> AppError {
    AppError::invalid_query_with_details(
        "Invalid escape sequence",
        error_details([
            ("kind", JsonValue::from("invalid_escape_sequence")),
            ("field", JsonValue::from("query")),
            ("value", JsonValue::from(value.into())),
            (
                "hint",
                JsonValue::from("escape_backslash_as_double_backslash"),
            ),
        ]),
    )
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
    fn parse_or(&mut self, depth: usize) -> Result<TagExpr, AppError> {
        validate_expr_depth(depth, TAG_DEPTH_ERROR)?;
        let mut terms = vec![self.parse_and(depth)?];
        while matches!(self.peek(), Some(TagToken::Or)) {
            self.next();
            terms.push(self.parse_and(depth)?);
        }
        if terms.len() == 1 {
            Ok(terms.remove(0))
        } else {
            Ok(TagExpr::Or(terms))
        }
    }

    /// Parses AND-level expression with implicit AND.
    fn parse_and(&mut self, depth: usize) -> Result<TagExpr, AppError> {
        validate_expr_depth(depth, TAG_DEPTH_ERROR)?;
        let mut terms = vec![self.parse_unary(depth)?];
        loop {
            if matches!(self.peek(), Some(TagToken::And)) {
                self.next();
                terms.push(self.parse_unary(depth)?);
                continue;
            }
            if matches!(
                self.peek(),
                Some(TagToken::Literal { .. }) | Some(TagToken::LParen) | Some(TagToken::Not)
            ) {
                terms.push(self.parse_unary(depth)?);
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
    fn parse_unary(&mut self, depth: usize) -> Result<TagExpr, AppError> {
        validate_expr_depth(depth, TAG_DEPTH_ERROR)?;
        if matches!(self.peek(), Some(TagToken::Not)) {
            self.next();
            return Ok(TagExpr::Not(Box::new(self.parse_unary(depth + 1)?)));
        }
        self.parse_primary(depth)
    }

    /// Parses primary expression.
    fn parse_primary(&mut self, depth: usize) -> Result<TagExpr, AppError> {
        validate_expr_depth(depth, TAG_DEPTH_ERROR)?;
        match self.next() {
            Some(TagToken::Literal { value, .. }) => {
                validate_tag_name(&value).map_err(|violation| {
                    invalid_tag_name_error(value.clone(), "query", violation)
                })?;
                Ok(TagExpr::Tag(value))
            }
            Some(TagToken::LParen) => {
                let expr = self.parse_or(depth + 1)?;
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

/// Parses a title-term expression.
pub(super) fn parse_term_expr(raw: &str) -> Result<TermExpr, AppError> {
    let tokens = lex_tag_expr(raw)?;
    if tokens.is_empty() {
        return Err(AppError::invalid_query("term group requires a value"));
    }
    let mut parser = TermExprParser::new(tokens);
    let expr = parser.parse_or(1)?;
    if parser.peek().is_some() {
        return Err(AppError::invalid_query("Invalid term expression"));
    }
    let normalized = normalize_term_expr(expr);
    validate_term_expr_limits(&normalized)?;
    Ok(normalized)
}

/// Parses `-(...)` value and rejects nested NOT expressions.
pub(super) fn parse_minus_term_expr(raw: &str) -> Result<TermExpr, AppError> {
    let expr = parse_term_expr(raw)?;
    if expr.contains_not() {
        return Err(AppError::invalid_query(
            "-(...) expression must not include NOT/!",
        ));
    }
    Ok(expr)
}

fn validate_term_expr_limits(expr: &TermExpr) -> Result<(), AppError> {
    validate_expr_depth(expr.max_depth(), TERM_DEPTH_ERROR)
}

fn normalize_term_expr(expr: TermExpr) -> TermExpr {
    normalize_expr(expr)
}

struct TermExprParser {
    tokens: Vec<TagToken>,
    index: usize,
}

impl TermExprParser {
    fn new(tokens: Vec<TagToken>) -> Self {
        Self { tokens, index: 0 }
    }

    fn peek(&self) -> Option<&TagToken> {
        self.tokens.get(self.index)
    }

    fn next(&mut self) -> Option<TagToken> {
        let token = self.tokens.get(self.index).cloned();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    fn parse_or(&mut self, depth: usize) -> Result<TermExpr, AppError> {
        validate_expr_depth(depth, TERM_DEPTH_ERROR)?;
        let mut terms = vec![self.parse_and(depth)?];
        while matches!(self.peek(), Some(TagToken::Or)) {
            self.next();
            terms.push(self.parse_and(depth)?);
        }
        if terms.len() == 1 {
            Ok(terms.remove(0))
        } else {
            Ok(TermExpr::Or(terms))
        }
    }

    fn parse_and(&mut self, depth: usize) -> Result<TermExpr, AppError> {
        validate_expr_depth(depth, TERM_DEPTH_ERROR)?;
        let mut terms = vec![self.parse_unary(depth)?];
        loop {
            if matches!(self.peek(), Some(TagToken::And)) {
                self.next();
                terms.push(self.parse_unary(depth)?);
                continue;
            }
            if matches!(
                self.peek(),
                Some(TagToken::Literal { .. }) | Some(TagToken::LParen) | Some(TagToken::Not)
            ) {
                terms.push(self.parse_unary(depth)?);
                continue;
            }
            break;
        }
        if terms.len() == 1 {
            Ok(terms.remove(0))
        } else {
            Ok(TermExpr::And(terms))
        }
    }

    fn parse_unary(&mut self, depth: usize) -> Result<TermExpr, AppError> {
        validate_expr_depth(depth, TERM_DEPTH_ERROR)?;
        if matches!(self.peek(), Some(TagToken::Not)) {
            self.next();
            return Ok(TermExpr::Not(Box::new(self.parse_unary(depth + 1)?)));
        }
        self.parse_primary(depth)
    }

    fn parse_primary(&mut self, depth: usize) -> Result<TermExpr, AppError> {
        validate_expr_depth(depth, TERM_DEPTH_ERROR)?;
        match self.next() {
            Some(TagToken::Literal { value, quoted }) => {
                if !quoted && value.contains(':') {
                    return Err(super::unknown_filter_prefix_error(&value));
                }
                Ok(TermExpr::Term(value))
            }
            Some(TagToken::LParen) => {
                let expr = self.parse_or(depth + 1)?;
                match self.next() {
                    Some(TagToken::RParen) => Ok(expr),
                    _ => Err(AppError::invalid_query(
                        "Unclosed parenthesis in term expression",
                    )),
                }
            }
            _ => Err(AppError::invalid_query("Invalid term expression")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_tag_expr;
    use crate::query::{EntryQuery, TagExpr};

    #[test]
    fn parse_tag_filters() {
        let query =
            EntryQuery::parse(Some("unread tag:tech -tag:misc"), Some("unread")).expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Tag("unread".to_string()),
            TagExpr::Tag("tech".to_string()),
            TagExpr::Not(Box::new(TagExpr::Tag("misc".to_string()))),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_single_token_tag_and_minus_tag_expressions() {
        let query =
            EntryQuery::parse(Some("tag:tech -tag:hot|rust"), Some("unread")).expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Tag("tech".to_string()),
            TagExpr::Not(Box::new(TagExpr::Or(vec![
                TagExpr::Tag("hot".to_string()),
                TagExpr::Tag("rust".to_string()),
            ]))),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn rejects_repeated_tag_filter_tokens() {
        for (raw, prefix, hint) in [
            (
                "tag:rust tag:async",
                "tag:",
                "merge_into_single_tag_expression",
            ),
            (
                "-tag:misc -tag:junk",
                "-tag:",
                "merge_into_single_minus_tag_expression",
            ),
        ] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
            assert_eq!(
                error.message(),
                format!("{prefix} cannot be specified multiple times")
            );
            let details = error.details().expect("details");
            assert_eq!(details["kind"], "duplicate_query_filter");
            assert_eq!(details["field"], "query");
            assert_eq!(details["value"], prefix);
            assert_eq!(details["hint"], hint);
        }
    }

    #[test]
    fn rejects_conflicting_tag_filters() {
        for raw in ["tag:rust -tag:rust", "tag:(rust & !rust)"] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
            assert_eq!(
                error.message(),
                "tag expression requires and excludes the same tag"
            );
        }
    }

    #[test]
    fn parse_tag_alias_expression() {
        let query = EntryQuery::parse(Some("tag:A|B|C"), Some("unread")).expect("query");
        let expected = normalize_tag_expr(TagExpr::Or(vec![
            TagExpr::Tag("A".to_string()),
            TagExpr::Tag("B".to_string()),
            TagExpr::Tag("C".to_string()),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_tag_parentheses_expression() {
        let query = EntryQuery::parse(Some("tag:A&(B|C)"), Some("unread")).expect("query");
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
    fn parses_adjacent_tag_after_group_as_implicit_and() {
        let query = EntryQuery::parse(Some("tag:(A|B)x"), Some("unread")).expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Or(vec![
                TagExpr::Tag("A".to_string()),
                TagExpr::Tag("B".to_string()),
            ]),
            TagExpr::Tag("x".to_string()),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_tag_precedence_expression() {
        let query = EntryQuery::parse(Some("tag:!A|B&C"), Some("unread")).expect("query");
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
        let query = EntryQuery::parse(Some("tag:A(B)"), Some("unread")).expect("query");
        let expected = normalize_tag_expr(TagExpr::And(vec![
            TagExpr::Tag("A".to_string()),
            TagExpr::Tag("B".to_string()),
        ]));
        assert_eq!(query.tag_expr, Some(expected));
    }

    #[test]
    fn parse_tag_keywords_case_insensitive() {
        let query =
            EntryQuery::parse(Some("tag:( a and b Or not c )"), Some("unread")).expect("query");
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
        let query = EntryQuery::parse(Some("-tag:C tag:A|B"), Some("unread")).expect("query");
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
        let query = EntryQuery::parse(Some("tag:A&B&C -tag:D|E"), Some("unread")).expect("query");
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
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
        }
    }

    #[test]
    fn rejects_invalid_tag_expression() {
        for raw in ["tag:(A|)", "tag:(A B", "tag:"] {
            let error = EntryQuery::parse(Some(raw), Some("unread")).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY");
        }
    }

    #[test]
    fn rejects_reserved_comma_in_tag_literal() {
        let error = EntryQuery::parse(Some(r#"tag:"rust,cli""#), Some("unread"))
            .expect_err("comma must be rejected in a tag name");

        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("error details");
        assert_eq!(details["kind"], "invalid_tag_name");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], "rust,cli");
        assert_eq!(details["hint"], "remove_reserved_comma");
    }

    #[test]
    fn rejects_control_character_in_tag_literal() {
        let error = EntryQuery::parse(Some("tag:\"line\nbreak\""), Some("unread"))
            .expect_err("control characters must be rejected in a tag name");

        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("error details");
        assert_eq!(details["kind"], "invalid_tag_name");
        assert_eq!(details["field"], "query");
        assert_eq!(details["hint"], "remove_control_characters");
    }

    #[test]
    fn rejects_tag_literal_over_64_unicode_characters() {
        let tag = "技".repeat(65);
        let raw = format!(r#"tag:"{tag}""#);
        let error = EntryQuery::parse(Some(&raw), Some("unread"))
            .expect_err("tag names over 64 characters must be rejected");

        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("error details");
        assert_eq!(details["kind"], "invalid_tag_name");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], tag);
        assert_eq!(details["hint"], "shorten_tag_name");
    }

    #[test]
    fn accepts_cjk_tag_literal_at_64_unicode_characters() {
        let tag = "技".repeat(64);
        let raw = format!(r#"tag:"{tag}""#);
        let query = EntryQuery::parse(Some(&raw), Some("unread"))
            .expect("64 CJK characters must be accepted");

        assert_eq!(query.tag_expr, Some(TagExpr::Tag(tag)));
    }

    #[test]
    fn accepts_unicode_and_quoted_special_tag_literals() {
        for (raw, expected) in [
            ("tag:日本語", "日本語"),
            (r#"tag:"機械 学習""#, "機械 学習"),
            (r#"tag:"rust🦀""#, "rust🦀"),
            (r#"tag:"分類/開発""#, "分類/開発"),
            (r#"tag:"a|b""#, "a|b"),
            (r#"tag:"and""#, "and"),
        ] {
            let query = EntryQuery::parse(Some(raw), Some("unread"))
                .expect("Unicode and quoted special characters must be accepted");

            assert_eq!(
                query.tag_expr,
                Some(TagExpr::Tag(expected.to_string())),
                "query: {raw}"
            );
        }
    }

    #[test]
    fn rejects_surrounding_whitespace_in_tag_literal() {
        let error = EntryQuery::parse(Some(r#"tag:" rust ""#), Some("unread"))
            .expect_err("surrounding whitespace must be rejected in a tag name");

        assert_eq!(error.code().as_str(), "INVALID_QUERY");
        let details = error.details().expect("error details");
        assert_eq!(details["kind"], "invalid_tag_name");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], " rust ");
        assert_eq!(details["hint"], "remove_surrounding_whitespace");
    }

    #[test]
    fn rejects_tag_expression_over_max_tokens() {
        let values = (0..65).map(|i| format!("t{i}")).collect::<Vec<_>>();
        let raw = format!("tag:{}", values.join("|"));
        let error = EntryQuery::parse(Some(&raw), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_tag_expression_over_max_depth() {
        let raw = format!("tag:{}A", "!".repeat(16));
        let error = EntryQuery::parse(Some(&raw), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn accepts_tag_expression_at_max_depth() {
        let raw = format!("tag:{}A", "!".repeat(15));
        let query = EntryQuery::parse(Some(&raw), Some("unread")).expect("query");
        assert!(query.tag_expr.is_some());
    }

    #[test]
    fn rejects_deeply_nested_tag_parentheses_without_stack_overflow() {
        let raw = format!("tag:{}A{}", "(".repeat(100_000), ")".repeat(100_000));
        let error = EntryQuery::parse(Some(&raw), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_deeply_nested_tag_not_without_stack_overflow() {
        let raw = format!("tag:{}A", "!".repeat(100_000));
        let error = EntryQuery::parse(Some(&raw), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }

    #[test]
    fn rejects_deeply_nested_term_parentheses_without_stack_overflow() {
        let raw = format!("{}A{}", "(".repeat(100_000), ")".repeat(100_000));
        let error = EntryQuery::parse(Some(&raw), Some("unread")).unwrap_err();
        assert_eq!(error.code().as_str(), "INVALID_QUERY");
    }
}
