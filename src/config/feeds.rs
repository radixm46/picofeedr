//! Feeds configuration parser for feeds.yaml.

use crate::error::{AppError, error_details};
use crate::tag::{duplicated_tag_names, merge_tag_names};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;
use serde_yaml_ng::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Parsed feeds configuration.
#[derive(Debug, Clone)]
pub struct FeedsConfig {
    /// Flattened feed list with inherited tags.
    pub feeds: Vec<FeedConfig>,
    /// Auto-tag rules defined in feeds.yaml.
    pub auto_tags: Vec<AutoTagRule>,
    /// Tag lists with logical paths for validation reporting.
    tag_lists: Vec<ScopedTagList>,
    /// Auto-tag rules with logical paths for validation reporting.
    auto_tag_rules: Vec<ScopedAutoTagRule>,
}

impl FeedsConfig {
    /// Loads feeds.yaml and returns a flattened configuration.
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let content = fs::read_to_string(path).map_err(|error| {
            AppError::config_with_details(
                format!("Failed to read feeds.yaml: {error}"),
                error_details([
                    ("path", JsonValue::from(path.to_string_lossy().to_string())),
                    ("hint", JsonValue::from("failed_to_read_feeds_yaml")),
                ]),
            )
        })?;
        let root: Value = serde_yaml_ng::from_str(&content).map_err(|error| {
            AppError::config_with_details(
                error.to_string(),
                error_details([
                    ("path", JsonValue::from(path.to_string_lossy().to_string())),
                    ("hint", JsonValue::from("invalid_yaml")),
                ]),
            )
        })?;
        let feeds_value = root.get("picofeedr").ok_or_else(|| {
            AppError::config_with_details(
                "feeds.yaml missing top-level 'picofeedr'",
                error_details([
                    ("path", JsonValue::from(path.to_string_lossy().to_string())),
                    ("hint", JsonValue::from("missing_top_level_picofeedr")),
                ]),
            )
        })?;
        let feeds_map = feeds_value.as_mapping().ok_or_else(|| {
            AppError::config_with_details(
                "feeds.yaml 'picofeedr' must be a mapping",
                error_details([
                    ("path", JsonValue::from(path.to_string_lossy().to_string())),
                    ("hint", JsonValue::from("picofeedr_must_be_mapping")),
                ]),
            )
        })?;
        let auto_tags = parse_auto_tags(feeds_map.get(Value::String("auto_tags".to_string())))?;
        let mut tag_lists = Vec::new();
        let mut auto_tag_rules = Vec::new();
        let mut feeds = Vec::new();
        flatten_group(
            feeds_value,
            &[],
            &[],
            "picofeedr",
            &mut feeds,
            &mut tag_lists,
            &mut auto_tag_rules,
        )?;
        Ok(Self {
            feeds,
            auto_tags,
            tag_lists,
            auto_tag_rules,
        })
    }

    /// Returns a unique list of all tags used by feeds.yaml.
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags = Vec::new();
        for feed in &self.feeds {
            tags = merge_tag_names(&tags, &feed.tags);
        }
        tags
    }

    /// Validates feeds.yaml semantics and returns a static validation report.
    pub fn validate(&self) -> ConfigCheckReport {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        let mut url_paths: HashMap<String, Vec<String>> = HashMap::new();
        for feed in &self.feeds {
            if feed.url.is_empty() {
                errors.push(ValidationIssue {
                    code: "EMPTY_FEED_URL".to_string(),
                    message: "feed url must not be empty".to_string(),
                    path: Some(format!("{}.url", feed.path)),
                });
            }
            url_paths
                .entry(feed.url.clone())
                .or_default()
                .push(feed.path.clone());
        }

        for scoped in &self.tag_lists {
            if scoped.tags.iter().any(|tag| tag.is_empty()) {
                errors.push(ValidationIssue {
                    code: "EMPTY_TAG_NAME".to_string(),
                    message: "tag name must not be empty".to_string(),
                    path: Some(scoped.path.clone()),
                });
            }
            let non_empty_tags = scoped
                .tags
                .iter()
                .filter(|tag| !tag.is_empty())
                .cloned()
                .collect::<Vec<_>>();
            for duplicated_tag in duplicated_tag_names(&non_empty_tags) {
                warnings.push(ValidationIssue {
                    code: "DUPLICATE_FEED_TAG".to_string(),
                    message: format!("duplicated feed tag '{duplicated_tag}'"),
                    path: Some(scoped.path.clone()),
                });
            }
        }

        for (url, paths) in url_paths {
            if url.is_empty() || paths.len() < 2 {
                continue;
            }
            for path in paths {
                errors.push(ValidationIssue {
                    code: "DUPLICATE_FEED_URL".to_string(),
                    message: format!("duplicated feed url '{url}'"),
                    path: Some(format!("{path}.url")),
                });
            }
        }

        for scoped in &self.auto_tag_rules {
            if scoped.rule.add_tags.is_empty() {
                errors.push(ValidationIssue {
                    code: "INVALID_AUTO_TAG_RULE".to_string(),
                    message: "auto tag rule requires at least one add_tags value".to_string(),
                    path: Some(format!("{}.add_tags", scoped.path)),
                });
            }
            if scoped.rule.add_tags.iter().any(|tag| tag.is_empty()) {
                errors.push(ValidationIssue {
                    code: "EMPTY_TAG_NAME".to_string(),
                    message: "tag name must not be empty".to_string(),
                    path: Some(format!("{}.add_tags", scoped.path)),
                });
            }
            if scoped.rule.title_regex.is_none() && scoped.rule.title_contains.is_none() {
                errors.push(ValidationIssue {
                    code: "INVALID_AUTO_TAG_RULE".to_string(),
                    message: "auto tag rule requires title_regex or title_contains".to_string(),
                    path: Some(scoped.path.clone()),
                });
            }
            if let Some(pattern) = &scoped.rule.title_regex
                && let Err(error) = Regex::new(pattern)
            {
                errors.push(ValidationIssue {
                    code: "INVALID_TITLE_REGEX".to_string(),
                    message: format!("invalid title_regex: {error}"),
                    path: Some(format!("{}.title_regex", scoped.path)),
                });
            }
        }

        ConfigCheckReport {
            valid: errors.is_empty(),
            errors,
            warnings,
            checked_feeds: self.feeds.len(),
        }
    }

    /// Returns a configuration error when feeds.yaml validation found blocking issues.
    pub fn ensure_valid_for_runtime(&self) -> Result<(), AppError> {
        let report = self.validate();
        if report.has_errors() {
            let first_issue = report.errors.first();
            return Err(AppError::config_with_details(
                format!(
                    "feeds.yaml validation failed with {} error(s); run `picofeedr sync --check` for details",
                    report.errors.len()
                ),
                error_details([
                    ("error_count", JsonValue::from(report.errors.len())),
                    (
                        "first_issue_code",
                        first_issue
                            .map(|issue| JsonValue::from(issue.code.clone()))
                            .unwrap_or(JsonValue::Null),
                    ),
                    (
                        "first_issue_path",
                        first_issue
                            .and_then(|issue| issue.path.clone().map(JsonValue::from))
                            .unwrap_or(JsonValue::Null),
                    ),
                    ("hint", JsonValue::from("run_sync_check")),
                ]),
            ));
        }
        Ok(())
    }
}

/// Single feed entry parsed from feeds.yaml.
#[derive(Debug, Clone)]
pub struct FeedConfig {
    /// Feed URL.
    pub url: String,
    /// Optional feed title from config.
    pub title: Option<String>,
    /// Tags inherited from groups plus feed-level tags.
    pub tags: Vec<String>,
    /// Logical path in feeds.yaml used for validation reporting.
    pub path: String,
    /// Feed-level tags before deduplication.
    pub declared_tags: Vec<String>,
    /// Effective auto-tag rules inherited for this feed.
    pub auto_tags: Vec<AutoTagRule>,
}

/// Auto-tag rule definition from feeds.yaml.
#[derive(Debug, Clone, Deserialize)]
pub struct AutoTagRule {
    /// Title regex pattern.
    pub title_regex: Option<String>,
    /// Title contains tokens.
    pub title_contains: Option<Vec<String>>,
    /// Tags to add when matched.
    pub add_tags: Vec<String>,
    /// Priority for rule ordering.
    pub priority: Option<i64>,
}

/// Auto-tag rule scoped with a logical path.
#[derive(Debug, Clone)]
struct ScopedAutoTagRule {
    /// Logical path for diagnostics.
    path: String,
    /// Auto-tag rule payload.
    rule: AutoTagRule,
}

/// Tag names scoped with a logical path.
#[derive(Debug, Clone)]
struct ScopedTagList {
    /// Logical path for diagnostics.
    path: String,
    /// Tag names declared at the path.
    tags: Vec<String>,
}

/// Static validation issue for feeds config check.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ValidationIssue {
    /// Machine-readable issue code.
    pub code: String,
    /// Human-readable issue details.
    pub message: String,
    /// Logical path in feeds.yaml where the issue was detected.
    pub path: Option<String>,
}

/// Static validation report for `sync --check`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ConfigCheckReport {
    /// True when no validation errors are present.
    pub valid: bool,
    /// Validation errors that should fail the command.
    pub errors: Vec<ValidationIssue>,
    /// Validation warnings that do not fail the command.
    pub warnings: Vec<ValidationIssue>,
    /// Number of feed entries checked.
    pub checked_feeds: usize,
}

impl ConfigCheckReport {
    /// Returns true when validation found any blocking errors.
    pub fn has_errors(&self) -> bool {
        !self.valid
    }
}

/// Parses auto_tag rules from YAML value.
fn parse_auto_tags(value: Option<&Value>) -> Result<Vec<AutoTagRule>, AppError> {
    match value {
        None => Ok(Vec::new()),
        Some(value) => {
            let mut rules: Vec<AutoTagRule> = serde_yaml_ng::from_value(value.clone())?;
            for rule in &mut rules {
                for tag in &mut rule.add_tags {
                    *tag = normalize_tag_name(tag);
                }
            }
            Ok(rules)
        }
    }
}

/// Flattens a single group node with inherited tags.
fn flatten_group(
    value: &Value,
    inherited: &[String],
    inherited_auto_tags: &[AutoTagRule],
    current_path: &str,
    out: &mut Vec<FeedConfig>,
    tag_lists: &mut Vec<ScopedTagList>,
    auto_tag_rules: &mut Vec<ScopedAutoTagRule>,
) -> Result<(), AppError> {
    let map = value
        .as_mapping()
        .ok_or_else(|| AppError::config("feed group must be a mapping"))?;
    let group_tags = map
        .get(Value::String("tags".to_string()))
        .and_then(|value| value.as_sequence())
        .map(parse_tag_list)
        .transpose()?
        .unwrap_or_default();
    if map.get(Value::String("tags".to_string())).is_some() {
        tag_lists.push(ScopedTagList {
            path: format!("{current_path}.tags"),
            tags: group_tags.clone(),
        });
    }
    let group_auto_tags = parse_auto_tags(map.get(Value::String("auto_tags".to_string())))?;
    append_scoped_rules(
        &format!("{current_path}.auto_tags"),
        &group_auto_tags,
        auto_tag_rules,
    );
    let merged_tags = merge_tags(inherited, &group_tags);
    let merged_auto_tags = merge_auto_tags(inherited_auto_tags, &group_auto_tags);

    if let Some(feeds_value) = map.get(Value::String("feeds".to_string())) {
        let feeds_seq = feeds_value
            .as_sequence()
            .ok_or_else(|| AppError::config("feeds entry must be a list"))?;
        for (index, feed_value) in feeds_seq.iter().enumerate() {
            let feed_map = feed_value
                .as_mapping()
                .ok_or_else(|| AppError::config("feed entry must be a mapping"))?;
            let url_value = feed_map
                .get(Value::String("url".to_string()))
                .and_then(|value| value.as_str())
                .ok_or_else(|| AppError::config("feed entry missing url"))?;
            let title = feed_map
                .get(Value::String("title".to_string()))
                .and_then(|value| value.as_str())
                .map(|value| value.to_string());
            let feed_tags = feed_map
                .get(Value::String("tags".to_string()))
                .and_then(|value| value.as_sequence())
                .map(parse_tag_list)
                .transpose()?
                .unwrap_or_default();
            if feed_map.get(Value::String("tags".to_string())).is_some() {
                tag_lists.push(ScopedTagList {
                    path: format!("{current_path}.feeds[{index}].tags"),
                    tags: feed_tags.clone(),
                });
            }
            let tags = merge_tags(&merged_tags, &feed_tags);
            out.push(FeedConfig {
                url: url_value.trim().to_string(),
                title,
                tags,
                path: format!("{current_path}.feeds[{index}]"),
                declared_tags: feed_tags,
                auto_tags: merged_auto_tags.clone(),
            });
        }
    }

    for (key, value) in map {
        if matches!(key, Value::String(name) if name == "tags" || name == "feeds" || name == "auto_tags")
        {
            continue;
        }
        let key = key
            .as_str()
            .ok_or_else(|| AppError::config("feeds group key must be a string"))?;
        let nested_path = format!("{current_path}.{key}");
        flatten_group(
            value,
            &merged_tags,
            &merged_auto_tags,
            &nested_path,
            out,
            tag_lists,
            auto_tag_rules,
        )?;
    }
    Ok(())
}

/// Parses a YAML tag list into strings.
fn parse_tag_list(values: &Vec<Value>) -> Result<Vec<String>, AppError> {
    let mut tags = Vec::new();
    for value in values {
        let tag = value
            .as_str()
            .ok_or_else(|| AppError::config("tag must be a string"))?;
        tags.push(normalize_tag_name(tag));
    }
    Ok(tags)
}

fn normalize_tag_name(tag: &str) -> String {
    tag.trim().to_string()
}

/// Merges two tag lists while preserving order and uniqueness.
fn merge_tags(base: &[String], extra: &[String]) -> Vec<String> {
    merge_tag_names(base, extra)
}

/// Merges inherited and local auto-tag rules while preserving order.
fn merge_auto_tags(base: &[AutoTagRule], extra: &[AutoTagRule]) -> Vec<AutoTagRule> {
    let mut merged = Vec::with_capacity(base.len() + extra.len());
    merged.extend(base.iter().cloned());
    merged.extend(extra.iter().cloned());
    merged
}

/// Appends scoped auto-tag rules for validation reporting.
fn append_scoped_rules(path_prefix: &str, rules: &[AutoTagRule], out: &mut Vec<ScopedAutoTagRule>) {
    for (index, rule) in rules.iter().enumerate() {
        out.push(ScopedAutoTagRule {
            path: format!("{path_prefix}[{index}]"),
            rule: rule.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::ConfigCheckReport;

    #[test]
    fn config_check_report_has_errors_reflects_validity() {
        let valid = ConfigCheckReport {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            checked_feeds: 1,
        };
        assert!(!valid.has_errors());

        let invalid = ConfigCheckReport {
            valid: false,
            errors: Vec::new(),
            warnings: Vec::new(),
            checked_feeds: 1,
        };
        assert!(invalid.has_errors());
    }
}
