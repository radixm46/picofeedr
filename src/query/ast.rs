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

fn escape_tag_literal(value: &str) -> String {
    value.replace('\\', "\\\\").replace(':', "\\:")
}

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
