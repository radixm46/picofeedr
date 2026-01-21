//! Feed reconciliation and CLI rendering.

use crate::config::feeds::{FeedConfig, FeedsConfig};
use crate::db::sqlite::SqliteStore;
use crate::db::{FeedInput, FeedRow};
use crate::error::AppError;
use crate::tag::TagManager;
use hex::ToHex;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Feed list JSON response.
#[derive(Debug, Serialize)]
pub struct FeedListResponse {
    /// Feed rows including config-derived tags.
    pub feeds: Vec<FeedListItem>,
}

/// Feed list item for JSON output.
#[derive(Debug, Serialize)]
pub struct FeedListItem {
    /// Internal database ID.
    pub id: i64,
    /// Stable feed key.
    pub feed_key: String,
    /// Feed URL.
    pub url: String,
    /// Optional title.
    pub title: Option<String>,
    /// Optional site URL.
    pub site_url: Option<String>,
    /// Optional author.
    pub author: Option<String>,
    /// Tags from feeds.yaml.
    pub tags: Vec<String>,
}

/// Config diff JSON response.
#[derive(Debug, Serialize)]
pub struct FeedConfigDiffResponse {
    /// Feeds only present in config.
    pub new_in_config: Vec<FeedConfigItem>,
    /// Feeds only present in the database.
    pub removed_from_config: Vec<FeedRemovedItem>,
    /// Feeds with tag changes.
    pub tag_changes: Vec<TagChange>,
}

/// Feed item present only in config.
#[derive(Debug, Serialize)]
pub struct FeedConfigItem {
    /// Stable feed key.
    pub feed_key: String,
    /// Feed URL.
    pub url: String,
    /// Optional title.
    pub title: Option<String>,
    /// Tags from feeds.yaml.
    pub tags: Vec<String>,
}

/// Feed item present only in the database.
#[derive(Debug, Serialize)]
pub struct FeedRemovedItem {
    /// Internal database ID.
    pub id: i64,
    /// Stable feed key.
    pub feed_key: String,
    /// Feed URL.
    pub url: String,
    /// Optional title.
    pub title: Option<String>,
}

/// Tag change between config and database metadata.
#[derive(Debug, Serialize)]
pub struct TagChange {
    /// Stable feed key.
    pub feed_key: String,
    /// Feed URL.
    pub url: String,
    /// Tags stored in database metadata.
    pub old_tags: Vec<String>,
    /// Tags from feeds.yaml.
    pub new_tags: Vec<String>,
}

/// Reconciles feeds.yaml with the database and ensures tag dictionary.
pub fn reconcile_feeds(
    store: &SqliteStore,
    config: &FeedsConfig,
    unread_tag: &str,
) -> Result<(), AppError> {
    let now = current_epoch();
    let mut all_tags = config.all_tags();
    for rule in &config.auto_tags {
        all_tags.extend(rule.add_tags.iter().cloned());
    }
    all_tags.push(unread_tag.to_string());
    let tag_manager = TagManager::new(store);
    tag_manager.ensure_tags(&all_tags)?;
    for feed in &config.feeds {
        let input = feed_input(feed);
        store.upsert_feed(&input, now)?;
    }
    Ok(())
}

/// Builds the feed list response with config-derived tags.
pub fn render_feed_list(config: &FeedsConfig, db_feeds: &[FeedRow]) -> FeedListResponse {
    let config_map = build_config_map(config);
    let feeds = db_feeds
        .iter()
        .map(|row| {
            let tags = config_map
                .get(&row.feed_key)
                .map(|feed| feed.tags.clone())
                .unwrap_or_default();
            FeedListItem {
                id: row.id,
                feed_key: row.feed_key.clone(),
                url: row.url.clone(),
                title: row.title.clone(),
                site_url: row.site_url.clone(),
                author: row.author.clone(),
                tags,
            }
        })
        .collect();
    FeedListResponse { feeds }
}

/// Builds the config vs DB diff for feeds.
pub fn diff_config_vs_db(config: &FeedsConfig, db_feeds: &[FeedRow]) -> FeedConfigDiffResponse {
    let config_map = build_config_map(config);
    let mut db_map: HashMap<String, &FeedRow> = HashMap::new();
    for feed in db_feeds {
        db_map.insert(feed.feed_key.clone(), feed);
    }

    let mut new_in_config = Vec::new();
    let mut tag_changes = Vec::new();
    for (feed_key, feed) in &config_map {
        if let Some(db_feed) = db_map.get(feed_key) {
            let old_tags = tags_from_meta(db_feed.meta_json.as_ref());
            if normalize_tags(&old_tags) != normalize_tags(&feed.tags) {
                tag_changes.push(TagChange {
                    feed_key: feed_key.clone(),
                    url: feed.url.clone(),
                    old_tags,
                    new_tags: feed.tags.clone(),
                });
            }
        } else {
            new_in_config.push(FeedConfigItem {
                feed_key: feed_key.clone(),
                url: feed.url.clone(),
                title: feed.title.clone(),
                tags: feed.tags.clone(),
            });
        }
    }

    let mut removed_from_config = Vec::new();
    for db_feed in db_feeds {
        if !config_map.contains_key(&db_feed.feed_key) {
            removed_from_config.push(FeedRemovedItem {
                id: db_feed.id,
                feed_key: db_feed.feed_key.clone(),
                url: db_feed.url.clone(),
                title: db_feed.title.clone(),
            });
        }
    }

    FeedConfigDiffResponse {
        new_in_config,
        removed_from_config,
        tag_changes,
    }
}

/// Builds a lookup map from feed_key to FeedConfig.
fn build_config_map(config: &FeedsConfig) -> HashMap<String, FeedConfig> {
    let mut map = HashMap::new();
    for feed in &config.feeds {
        let feed_key = feed_key_from_url(&feed.url);
        map.insert(feed_key, feed.clone());
    }
    map
}

/// Builds a FeedInput payload from a FeedConfig entry.
fn feed_input(feed: &FeedConfig) -> FeedInput {
    FeedInput {
        feed_key: feed_key_from_url(&feed.url),
        url: feed.url.clone(),
        title: feed.title.clone(),
        author: None,
        site_url: None,
        meta_json: Some(json!({"tags": feed.tags}).to_string()),
    }
}

/// Generates a stable feed key from the feed URL.
pub fn feed_key_from_url(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    digest.encode_hex::<String>()
}

/// Extracts tags from feed metadata JSON.
fn tags_from_meta(meta_json: Option<&String>) -> Vec<String> {
    let Some(raw) = meta_json else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(tags_value) = value.get("tags") else {
        return Vec::new();
    };
    let Some(tags_array) = tags_value.as_array() else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    for tag in tags_array {
        if let Some(tag_str) = tag.as_str() {
            tags.push(tag_str.to_string());
        }
    }
    tags
}

/// Normalizes tags for comparison by sorting and deduping.
fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut sorted = tags.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted
}

/// Returns the current epoch time in seconds.
/// Returns the current epoch time in seconds.
fn current_epoch() -> i64 {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
    duration.as_secs() as i64
}
