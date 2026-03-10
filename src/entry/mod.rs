//! Entry response types and public API surface.

mod list;
mod mark;
mod view;

use schemars::JsonSchema;
use serde::Serialize;

pub use list::list_entries;
pub use mark::mark_entries;
pub use view::view_entry;

/// Entry summary for list responses.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntrySummary {
    /// Entry id.
    pub entry_id: String,
    /// Feed id.
    pub feed_id: String,
    /// Entry title.
    pub title: Option<String>,
    /// Entry link.
    pub link: Option<String>,
    /// Published time.
    pub published_at: Option<i64>,
    /// First seen time.
    pub first_seen_at: i64,
    /// Tags applied to the entry.
    pub tags: Vec<String>,
}

/// Entry enclosure payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryEnclosure {
    /// Enclosure URL.
    pub url: String,
    /// MIME type.
    pub mime_type: Option<String>,
    /// Length in bytes.
    pub length: Option<i64>,
}

/// Entry detail payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryDetail {
    /// Entry id.
    pub entry_id: String,
    /// Feed id.
    pub feed_id: String,
    /// Feed title.
    pub feed_title: Option<String>,
    /// Entry title.
    pub title: Option<String>,
    /// Entry link.
    pub link: Option<String>,
    /// Entry author.
    pub author: Option<String>,
    /// Published time.
    pub published_at: Option<i64>,
    /// First seen time.
    pub first_seen_at: i64,
    /// Entry content body.
    pub content: Option<String>,
    /// Content type.
    pub content_type: Option<String>,
    /// Tags applied to the entry.
    pub tags: Vec<String>,
    /// Enclosure list.
    pub enclosures: Vec<EntryEnclosure>,
}

/// Entry list response payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntryListResponse {
    /// Total hits for the query.
    pub total_count: i64,
    /// Page items.
    pub items: Vec<EntrySummary>,
    /// Feed dictionary for feed id to title mapping.
    pub feeds: Vec<FeedSummary>,
    /// Cursor for the next page.
    pub next_page_token: Option<String>,
    /// Revision captured when the list was fetched.
    pub revision: i64,
    /// Write timestamp captured when the list was fetched.
    pub last_write_at: Option<i64>,
}

/// Feed dictionary item used by entry list responses.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeedSummary {
    /// Feed id.
    pub feed_id: String,
    /// Feed title.
    pub title: Option<String>,
}
