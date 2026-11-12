//! Auto-tag rule compilation and matching.

use crate::config::feeds::AutoTagRule;
use crate::error::AppError;
use regex::Regex;

/// Auto-tag rule compiled for matching.
#[derive(Debug, Clone)]
pub(crate) struct CompiledRule {
    pub(crate) regex: Option<Regex>,
    pub(crate) contains: Vec<String>,
    pub(crate) add_tags: Vec<String>,
    pub(crate) priority: i64,
}

/// Compiles auto-tag rules into matchable structures.
pub(crate) fn compile_auto_tags(rules: &[AutoTagRule]) -> Result<Vec<CompiledRule>, AppError> {
    let mut compiled = Vec::new();
    for rule in rules {
        let regex = match &rule.title_regex {
            Some(pattern) => Some(
                Regex::new(pattern)
                    .map_err(|error| AppError::config(format!("Invalid title_regex: {error}")))?,
            ),
            None => None,
        };
        let contains = rule
            .title_contains
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|value| value.to_lowercase())
            .collect();
        compiled.push(CompiledRule {
            regex,
            contains,
            add_tags: rule.add_tags.clone(),
            priority: rule.priority.unwrap_or(0),
        });
    }
    compiled.sort_by_key(|rule| rule.priority);
    Ok(compiled)
}

/// Evaluates auto-tag rules against an entry title.
pub(crate) fn match_auto_tags(title: &str, rules: &[CompiledRule]) -> Vec<String> {
    let lower = title.to_lowercase();
    let mut tags = Vec::new();
    for rule in rules {
        let mut matched = false;
        if let Some(regex) = &rule.regex {
            matched |= regex.is_match(title);
        }
        if !rule.contains.is_empty() {
            matched |= rule.contains.iter().any(|token| lower.contains(token));
        }
        if matched {
            tags.extend(rule.add_tags.iter().cloned());
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::{compile_auto_tags, match_auto_tags};
    use crate::config::feeds::AutoTagRule;

    #[test]
    fn title_regex_matching_is_case_sensitive_by_default() {
        for title_contains in [None, Some(vec![])] {
            let rules = compile_auto_tags(&[AutoTagRule {
                title_regex: Some("steam".to_string()),
                title_contains,
                add_tags: vec!["regex".to_string()],
                priority: None,
            }])
            .expect("compile rules");

            assert_eq!(match_auto_tags("steam weekly", &rules), vec!["regex"]);
            assert!(match_auto_tags("Steam Weekly", &rules).is_empty());
        }
    }

    #[test]
    fn empty_contains_list_matches_nothing_but_empty_literal_matches_any_title() {
        for (contains, expected) in [(vec![], vec![]), (vec![String::new()], vec!["contains"])] {
            let rules = compile_auto_tags(&[AutoTagRule {
                title_regex: None,
                title_contains: Some(contains),
                add_tags: vec!["contains".to_string()],
                priority: None,
            }])
            .expect("compile rules");
            assert_eq!(match_auto_tags("Any title", &rules), expected);
        }
    }

    #[test]
    fn title_contains_matching_is_case_insensitive() {
        for (filter, title) in [
            ("Steam", "sTEAM weekly"),
            ("İ", "i\u{307} weekly"),
            ("i\u{307}", "İ weekly"),
        ] {
            let rules = compile_auto_tags(&[AutoTagRule {
                title_regex: None,
                title_contains: Some(vec![filter.to_string()]),
                add_tags: vec!["contains".to_string()],
                priority: None,
            }])
            .expect("compile rules");

            assert_eq!(
                match_auto_tags(title, &rules),
                vec!["contains"],
                "{filter:?}, {title:?}"
            );
            assert!(
                match_auto_tags("unrelated", &rules).is_empty(),
                "{filter:?}"
            );
        }
    }
}
