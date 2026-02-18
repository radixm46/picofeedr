//! Sync data structures.

use crate::db::EntryContentInput;
use crate::error::AppError;
use schemars::JsonSchema;
use serde::Serialize;

use super::autotag::CompiledRule;

/// Sync result summary.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SyncSummary {
    /// Sync status.
    pub status: SyncStatus,
    /// Number of feeds fetched.
    pub fetched_feed_count: usize,
    /// Number of failed feeds.
    pub failed_feed_count: usize,
    /// Number of new entries ingested.
    pub new_entry_count: usize,
    /// Elapsed time in milliseconds.
    pub duration_ms: u64,
    /// Sync errors for failed feeds.
    pub errors: Vec<SyncError>,
}

/// Sync error entry for failed feeds.
#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct SyncError {
    /// Feed URL that failed.
    pub feed_url: String,
    /// Error code.
    pub code: SyncErrorCode,
    /// Error message.
    pub message: String,
    /// Whether the caller should retry.
    pub retryable: bool,
}

/// Sync status values.
#[derive(Debug, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Completed,
    PartialFailed,
    Failed,
}

impl SyncStatus {
    /// Returns the display label for plain output.
    pub fn as_str(self) -> &'static str {
        match self {
            SyncStatus::Completed => "completed",
            SyncStatus::PartialFailed => "partial_failed",
            SyncStatus::Failed => "failed",
        }
    }
}

/// Sync error code values.
#[derive(Debug, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
pub enum SyncErrorCode {
    #[serde(rename = "FETCH_FAILED")]
    FetchFailed,
    #[serde(rename = "PARSE_FAILED")]
    ParseFailed,
}

impl SyncErrorCode {
    /// Returns the display label for plain output.
    pub fn as_str(self) -> &'static str {
        match self {
            SyncErrorCode::FetchFailed => "FETCH_FAILED",
            SyncErrorCode::ParseFailed => "PARSE_FAILED",
        }
    }
}

/// Sync progress event for interactive plain output.
#[derive(Debug, Clone)]
pub enum SyncProgressEvent {
    /// Sync execution started with the total feed count.
    Start { total_feeds: usize },
    /// A feed started processing.
    FeedStart {
        index: usize,
        total_feeds: usize,
        url: String,
    },
    /// A feed finished successfully.
    FeedOk {
        index: usize,
        total_feeds: usize,
        url: String,
        entries: usize,
    },
    /// A feed failed with a non-fatal sync error.
    FeedError {
        index: usize,
        total_feeds: usize,
        url: String,
        code: SyncErrorCode,
        retryable: bool,
    },
}

impl SyncError {
    /// Builds a fetch error entry.
    pub(crate) fn fetch(feed_url: &str, message: String, retryable: bool) -> Self {
        Self {
            feed_url: feed_url.to_string(),
            code: SyncErrorCode::FetchFailed,
            message,
            retryable,
        }
    }

    /// Builds a parse error entry.
    pub(crate) fn parse(feed_url: &str, message: String) -> Self {
        Self {
            feed_url: feed_url.to_string(),
            code: SyncErrorCode::ParseFailed,
            message,
            retryable: false,
        }
    }
}

/// Sync target with feed metadata and tags.
#[derive(Debug, Clone)]
pub(crate) struct SyncTarget {
    pub(crate) feed_id: String,
    pub(crate) url: String,
    pub(crate) tags: Vec<String>,
    pub(crate) auto_tag_rules: Vec<CompiledRule>,
    pub(crate) index: usize,
    pub(crate) total_feeds: usize,
}

/// Parsed feed result from fetch workers.
#[derive(Debug)]
pub(crate) struct SyncResult {
    pub(crate) entries: Vec<SyncEntry>,
}

/// Normalized entry with tags and content payload.
#[derive(Debug)]
pub(crate) struct SyncEntry {
    pub(crate) feed_id: String,
    pub(crate) entry: PendingEntry,
    pub(crate) content: Option<EntryContentInput>,
    /// Content payload for filesystem storage.
    pub(crate) content_payload: Option<String>,
    pub(crate) tags: Vec<String>,
}

/// Planned content storage for sync entries.
#[derive(Debug)]
pub(crate) struct EntryContentPlan {
    pub(crate) content: Option<EntryContentInput>,
    pub(crate) payload: Option<String>,
}

/// Pending entry data before feed primary key resolution.
#[derive(Debug)]
pub(crate) struct PendingEntry {
    pub(crate) entry_id: String,
    pub(crate) source_id: Option<String>,
    pub(crate) link: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) published_at: Option<i64>,
    pub(crate) updated_at: Option<i64>,
    pub(crate) first_seen_at: i64,
    pub(crate) meta_json: Option<String>,
}

impl PendingEntry {
    /// Builds an EntryInput by attaching feed primary key.
    pub(crate) fn with_feed_pk(self, feed_pk: i64) -> crate::db::EntryInput {
        crate::db::EntryInput {
            entry_id: self.entry_id,
            feed_pk,
            source_id: self.source_id,
            link: self.link,
            title: self.title,
            author: self.author,
            published_at: self.published_at,
            updated_at: self.updated_at,
            first_seen_at: self.first_seen_at,
            meta_json: self.meta_json,
        }
    }
}

/// Worker result returned from fetch threads.
#[derive(Debug)]
pub(crate) enum WorkerResult {
    /// Feed processing started.
    Started {
        index: usize,
        total_feeds: usize,
        url: String,
    },
    /// Parsed feed result.
    Ok {
        index: usize,
        total_feeds: usize,
        url: String,
        result: SyncResult,
    },
    /// Non-fatal sync error for a feed.
    Error {
        index: usize,
        total_feeds: usize,
        url: String,
        error: SyncError,
    },
    /// Fatal error that should abort sync.
    Fatal(AppError),
}
