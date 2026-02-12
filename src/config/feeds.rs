//! Feeds configuration parser for feeds.yaml.

use crate::error::AppError;
use serde::Deserialize;
use serde::Serialize;
use serde_yaml_ng::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Parsed feeds configuration.
#[derive(Debug, Clone)]
pub struct FeedsConfig {
    /// Flattened feed list with inherited tags.
    pub feeds: Vec<FeedConfig>,
    /// Auto-tag rules defined in feeds.yaml.
    #[allow(dead_code)]
    pub auto_tags: Vec<AutoTagRule>,
}

impl FeedsConfig {
    /// Loads feeds.yaml and returns a flattened configuration.
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let content = fs::read_to_string(path)
            .map_err(|error| AppError::config(format!("Failed to read feeds.yaml: {error}")))?;
        let root: Value = serde_yaml_ng::from_str(&content)?;
        let feeds_value = root
            .get("feeds")
            .ok_or_else(|| AppError::config("feeds.yaml missing top-level 'feeds'"))?;
        let feeds_map = feeds_value
            .as_mapping()
            .ok_or_else(|| AppError::config("feeds.yaml 'feeds' must be a mapping"))?;
        let auto_tags = parse_auto_tags(feeds_map.get(Value::String("auto_tags".to_string())))?;
        let mut feeds = Vec::new();
        flatten_groups(feeds_value, &[], "feeds", &mut feeds)?;
        Ok(Self { feeds, auto_tags })
    }

    /// Returns a unique list of all tags used by feeds.yaml.
    pub fn all_tags(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut tags = Vec::new();
        for feed in &self.feeds {
            for tag in &feed.tags {
                if seen.insert(tag.clone()) {
                    tags.push(tag.clone());
                }
            }
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

            for duplicated_tag in duplicated_values(&feed.declared_tags) {
                warnings.push(ValidationIssue {
                    code: "DUPLICATE_FEED_TAG".to_string(),
                    message: format!("duplicated feed tag '{duplicated_tag}'"),
                    path: Some(format!("{}.tags", feed.path)),
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

        for (index, rule) in self.auto_tags.iter().enumerate() {
            if rule.add_tags.is_empty() {
                errors.push(ValidationIssue {
                    code: "INVALID_AUTO_TAG_RULE".to_string(),
                    message: "auto tag rule requires at least one add_tags value".to_string(),
                    path: Some(format!("feeds.auto_tags[{index}].add_tags")),
                });
            }
            if rule.title_regex.is_none() && rule.title_contains.is_none() {
                errors.push(ValidationIssue {
                    code: "INVALID_AUTO_TAG_RULE".to_string(),
                    message: "auto tag rule requires title_regex or title_contains".to_string(),
                    path: Some(format!("feeds.auto_tags[{index}]")),
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
}

/// Auto-tag rule definition from feeds.yaml.
#[derive(Debug, Clone, Deserialize)]
pub struct AutoTagRule {
    /// Title regex pattern.
    #[allow(dead_code)]
    pub title_regex: Option<String>,
    /// Title contains tokens.
    #[allow(dead_code)]
    pub title_contains: Option<Vec<String>>,
    /// Tags to add when matched.
    #[allow(dead_code)]
    pub add_tags: Vec<String>,
    /// Priority for rule ordering.
    #[allow(dead_code)]
    pub priority: Option<i64>,
}

/// Static validation issue for feeds config check.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    /// Machine-readable issue code.
    pub code: String,
    /// Human-readable issue details.
    pub message: String,
    /// Logical path in feeds.yaml where the issue was detected.
    pub path: Option<String>,
}

/// Static validation report for `feeds --config-check`.
#[derive(Debug, Clone, Serialize)]
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

/// Parses auto_tag rules from YAML value.
fn parse_auto_tags(value: Option<&Value>) -> Result<Vec<AutoTagRule>, AppError> {
    match value {
        None => Ok(Vec::new()),
        Some(value) => {
            let rules: Vec<AutoTagRule> = serde_yaml_ng::from_value(value.clone())?;
            Ok(rules)
        }
    }
}

/// Flattens nested feed groups into a list of feed configs.
fn flatten_groups(
    value: &Value,
    inherited: &[String],
    current_path: &str,
    out: &mut Vec<FeedConfig>,
) -> Result<(), AppError> {
    let map = value
        .as_mapping()
        .ok_or_else(|| AppError::config("feeds.yaml 'feeds' must be a mapping"))?;
    for (key, group) in map {
        if matches!(key, Value::String(name) if current_path == "feeds" && name == "auto_tags") {
            continue;
        }
        let key = key
            .as_str()
            .ok_or_else(|| AppError::config("feeds group key must be a string"))?;
        let group_path = format!("{current_path}.{key}");
        flatten_group(group, inherited, &group_path, out)?;
    }
    Ok(())
}

/// Flattens a single group node with inherited tags.
fn flatten_group(
    value: &Value,
    inherited: &[String],
    current_path: &str,
    out: &mut Vec<FeedConfig>,
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
    let merged_tags = merge_tags(inherited, &group_tags);

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
            let tags = merge_tags(&merged_tags, &feed_tags);
            out.push(FeedConfig {
                url: url_value.trim().to_string(),
                title,
                tags,
                path: format!("{current_path}.feeds[{index}]"),
                declared_tags: feed_tags,
            });
        }
    }

    for (key, value) in map {
        if matches!(key, Value::String(name) if name == "tags" || name == "feeds") {
            continue;
        }
        let key = key
            .as_str()
            .ok_or_else(|| AppError::config("feeds group key must be a string"))?;
        let nested_path = format!("{current_path}.{key}");
        flatten_group(value, &merged_tags, &nested_path, out)?;
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
        tags.push(tag.to_string());
    }
    Ok(tags)
}

/// Merges two tag lists while preserving order and uniqueness.
fn merge_tags(base: &[String], extra: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for tag in base.iter().chain(extra.iter()) {
        if seen.insert(tag.clone()) {
            merged.push(tag.clone());
        }
    }
    merged
}

/// Returns duplicated values while preserving first-seen order.
fn duplicated_values(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut duplicates = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        if !seen.insert(value.clone()) && duplicates.insert(value.clone()) {
            result.push(value.clone());
        }
    }
    result
}
