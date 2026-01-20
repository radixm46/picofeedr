//! Feeds configuration parser for feeds.yaml.

use crate::error::AppError;
use serde::Deserialize;
use serde_yaml_ng::Value;
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
        let mut feeds = Vec::new();
        flatten_groups(feeds_value, &[], &mut feeds)?;
        let auto_tags = parse_auto_tags(root.get("auto_tags"))?;
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
    out: &mut Vec<FeedConfig>,
) -> Result<(), AppError> {
    let map = value
        .as_mapping()
        .ok_or_else(|| AppError::config("feeds.yaml 'feeds' must be a mapping"))?;
    for (_, group) in map {
        flatten_group(group, inherited, out)?;
    }
    Ok(())
}

/// Flattens a single group node with inherited tags.
fn flatten_group(
    value: &Value,
    inherited: &[String],
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
        for feed_value in feeds_seq {
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
            });
        }
    }

    for (key, value) in map {
        if matches!(key, Value::String(name) if name == "tags" || name == "feeds") {
            continue;
        }
        flatten_group(value, &merged_tags, out)?;
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
