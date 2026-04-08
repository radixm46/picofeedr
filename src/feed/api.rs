//! Feed response rendering for CLI output.

use crate::config::feeds::{FeedConfig, FeedsConfig};
use crate::db::FeedRow;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::HashMap;

use super::identity::feed_id_from_url;

/// Feed list JSON response.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeedListResponse {
    /// Feed rows including config-derived tags.
    pub feeds: Vec<FeedListItem>,
}

/// Feed list item for JSON output.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeedListItem {
    /// Stable public feed id.
    pub feed_id: String,
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

/// Builds the feed list response with config-derived tags.
pub fn build_feed_list_response(config: &FeedsConfig, db_feeds: &[FeedRow]) -> FeedListResponse {
    let config_map = build_config_map(config);
    let feeds = db_feeds
        .iter()
        .map(|row| {
            let tags = config_map
                .get(&row.feed_id)
                .map(|feed| feed.tags.clone())
                .unwrap_or_default();
            FeedListItem {
                feed_id: row.feed_id.clone(),
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

/// Builds a lookup map from feed_id to FeedConfig.
fn build_config_map(config: &FeedsConfig) -> HashMap<String, FeedConfig> {
    let mut map = HashMap::new();
    for feed in &config.feeds {
        let feed_id = feed_id_from_url(&feed.url);
        map.insert(feed_id, feed.clone());
    }
    map
}
