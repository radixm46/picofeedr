use super::TagExpr;
use crate::error::AppError;
use std::collections::HashSet;

const MAX_TAG_TOKENS: usize = 64;
const MAX_TAG_AST_DEPTH: usize = 16;

/// Parses a tag expression.
pub(super) fn parse_tag_expr(raw: &str) -> Result<TagExpr, AppError> {
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
    if expr.max_depth() > MAX_TAG_AST_DEPTH {
        return Err(AppError::invalid_query("Tag expression exceeds max depth"));
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
    // TODO: If parser constraints ever change, reject or model empty variadic expressions explicitly.
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
    use super::normalize_tag_expr;
    use crate::query::{EntryQuery, TagExpr};

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
