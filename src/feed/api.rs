//! Feed response rendering for CLI output.

use crate::db::FeedRow;
use schemars::JsonSchema;
use serde::Serialize;

/// Feed list JSON response.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeedListResponse {
    /// Feed rows stored in the database.
    pub feeds: Vec<FeedListItem>,
}

/// Feed list item for JSON output.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeedListItem {
    /// Stable public feed id.
    pub feed_id: String,
    /// Feed URL.
    pub url: String,
    /// Feed title recorded in the database.
    pub title: Option<String>,
    /// Last observed feed site URL.
    pub site_url: Option<String>,
    /// Last observed feed author.
    pub author: Option<String>,
}

/// Builds the feed list response from database rows.
pub fn build_feed_list_response(db_feeds: &[FeedRow]) -> FeedListResponse {
    let feeds = db_feeds
        .iter()
        .map(|row| FeedListItem {
            feed_id: row.feed_id.clone(),
            url: row.url.clone(),
            title: row.title.clone(),
            site_url: row.site_url.clone(),
            author: row.author.clone(),
        })
        .collect();
    FeedListResponse { feeds }
}
