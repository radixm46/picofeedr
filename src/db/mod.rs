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

/// Entry insert payload for the database.
#[derive(Debug, Clone)]
pub struct EntryInput {
    /// Stable entry key.
    pub entry_key: String,
    /// Feed foreign key.
    pub feed_id: i64,
    /// Source identifier from the feed.
    pub source_id: Option<String>,
    /// Link URL.
    pub link: Option<String>,
    /// Entry title.
    pub title: Option<String>,
    /// Entry author.
    pub author: Option<String>,
    /// Published timestamp (epoch seconds).
    pub published_at: Option<i64>,
    /// Updated timestamp (epoch seconds).
    pub updated_at: Option<i64>,
    /// First seen timestamp (epoch seconds).
    pub first_seen_at: i64,
    /// JSON metadata.
    pub meta_json: Option<String>,
}

/// Entry content insert payload.
#[derive(Debug, Clone)]
pub struct EntryContentInput {
    /// Storage mode.
    pub storage: String,
    /// Storage reference for filesystem content.
    pub reference: Option<String>,
    /// Content type.
    pub content_type: Option<String>,
    /// Content payload (DB storage only).
    pub content: Option<String>,
}

/// Result of inserting an entry.
#[derive(Debug, Clone)]
pub struct EntryInsertResult {
    /// Entry database ID.
    pub entry_id: i64,
    /// Whether this entry was newly inserted.
    pub inserted: bool,
}
