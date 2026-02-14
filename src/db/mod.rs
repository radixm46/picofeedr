//! Database types and helpers.

pub mod migrate;
pub mod sqlite;

/// Feed row as stored in the database.
#[derive(Debug, Clone)]
pub struct FeedRow {
    /// Internal feed primary key.
    pub feed_pk: i64,
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
    /// Feed foreign key (`feeds.id`).
    pub feed_pk: i64,
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
    pub storage: EntryContentStorage,
    /// Storage reference for filesystem content.
    pub reference: Option<String>,
    /// Content type.
    pub content_type: Option<String>,
    /// Content payload (DB storage only).
    pub content: Option<String>,
}

/// Storage mode for entry contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryContentStorage {
    /// Store content in SQLite.
    Db,
    /// Store content in filesystem.
    Fs,
    /// No content stored.
    None,
}

impl EntryContentStorage {
    /// Returns the storage value for persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            EntryContentStorage::Db => "db",
            EntryContentStorage::Fs => "fs",
            EntryContentStorage::None => "none",
        }
    }
}

impl std::str::FromStr for EntryContentStorage {
    type Err = ();

    /// Parses the storage value from persistence.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "db" => Ok(EntryContentStorage::Db),
            "fs" => Ok(EntryContentStorage::Fs),
            "none" => Ok(EntryContentStorage::None),
            _ => Err(()),
        }
    }
}

/// Result of inserting an entry.
#[derive(Debug, Clone)]
pub struct EntryInsertResult {
    /// Internal entry primary key.
    pub entry_pk: i64,
    /// Whether this entry was newly inserted.
    pub inserted: bool,
}
