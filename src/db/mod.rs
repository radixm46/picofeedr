//! Database types and helpers.

pub mod migrate;
pub mod sqlite;

/// Feed row as stored in the database.
#[derive(Debug, Clone)]
pub struct FeedRow {
    /// Internal database ID.
    pub id: i64,
    /// Stable feed key.
    pub feed_key: String,
    /// Feed URL.
    pub url: String,
    /// Optional title.
    pub title: Option<String>,
    /// Optional author.
    pub author: Option<String>,
    /// Optional site URL.
    pub site_url: Option<String>,
    /// Optional JSON metadata.
    pub meta_json: Option<String>,
}

/// Feed input for upserts.
#[derive(Debug, Clone)]
pub struct FeedInput {
    /// Stable feed key.
    pub feed_key: String,
    /// Feed URL.
    pub url: String,
    /// Optional title.
    pub title: Option<String>,
    /// Optional author.
    pub author: Option<String>,
    /// Optional site URL.
    pub site_url: Option<String>,
    /// Optional JSON metadata.
    pub meta_json: Option<String>,
}
