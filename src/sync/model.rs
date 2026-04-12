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
    /// Stable feed id.
    pub feed_id: String,
    /// Feed name from config when available.
    pub feed_name: Option<String>,
    /// Feed URL that failed.
    pub feed_url: String,
    /// 1-based feed position within the sync job.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) index: usize,
    /// Total feed count for the sync job.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) total_feeds: usize,
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

    /// Returns true when the sync result should surface as a non-fatal warning.
    pub fn is_warning(self) -> bool {
        matches!(self, SyncStatus::PartialFailed | SyncStatus::Failed)
    }
}

/// Sync error code values.
#[derive(Debug, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum SyncErrorCode {
    #[serde(rename = "FETCH_FAILED")]
    FetchFailed,
    #[serde(rename = "PARSE_FAILED")]
    ParseFailed,
    #[serde(rename = "INGEST_FAILED")]
    IngestFailed,
}

impl SyncErrorCode {
    /// Returns the display label for plain output.
    pub fn as_str(self) -> &'static str {
        match self {
            SyncErrorCode::FetchFailed => "FETCH_FAILED",
            SyncErrorCode::ParseFailed => "PARSE_FAILED",
            SyncErrorCode::IngestFailed => "INGEST_FAILED",
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
    /// Returns the 1-based feed position and total feed count for plain progress output.
    pub fn progress_position(&self) -> (usize, usize) {
        (self.index, self.total_feeds)
    }

    /// Builds a fetch error entry.
    pub(crate) fn fetch(
        feed_id: &str,
        feed_name: Option<&str>,
        feed_url: &str,
        index: usize,
        total_feeds: usize,
        message: String,
        retryable: bool,
    ) -> Self {
        Self {
            feed_id: feed_id.to_string(),
            feed_name: feed_name.map(ToOwned::to_owned),
            feed_url: feed_url.to_string(),
            index,
            total_feeds,
            code: SyncErrorCode::FetchFailed,
            message,
            retryable,
        }
    }

    /// Builds a parse error entry.
    pub(crate) fn parse(
        feed_id: &str,
        feed_name: Option<&str>,
        feed_url: &str,
        index: usize,
        total_feeds: usize,
        message: String,
    ) -> Self {
        Self {
            feed_id: feed_id.to_string(),
            feed_name: feed_name.map(ToOwned::to_owned),
            feed_url: feed_url.to_string(),
            index,
            total_feeds,
            code: SyncErrorCode::ParseFailed,
            message,
            retryable: false,
        }
    }

    /// Builds an ingest error entry.
    pub(crate) fn ingest(
        feed_id: &str,
        feed_name: Option<&str>,
        feed_url: &str,
        index: usize,
        total_feeds: usize,
        message: String,
    ) -> Self {
        Self {
            feed_id: feed_id.to_string(),
            feed_name: feed_name.map(ToOwned::to_owned),
            feed_url: feed_url.to_string(),
            index,
            total_feeds,
            code: SyncErrorCode::IngestFailed,
            message,
            retryable: false,
        }
    }
}

/// Sync target with feed metadata and tags.
#[derive(Debug, Clone)]
pub(crate) struct SyncTarget {
    pub(crate) feed_id: String,
    pub(crate) feed_name: Option<String>,
    pub(crate) url: String,
    pub(crate) tags: Vec<String>,
    pub(crate) auto_tag_rules: Vec<CompiledRule>,
    pub(crate) index: usize,
    pub(crate) total_feeds: usize,
}

/// Parsed feed result from fetch workers.
#[derive(Debug)]
pub(crate) struct SyncResult {
    pub(crate) feed_id: String,
    pub(crate) feed_name: Option<String>,
    pub(crate) feed_url: String,
    pub(crate) index: usize,
    pub(crate) total_feeds: usize,
    pub(crate) feed_metadata: FeedMetadata,
    pub(crate) entries: Vec<SyncEntry>,
}

/// Feed-level metadata observed during sync.
#[derive(Debug, Default)]
pub(crate) struct FeedMetadata {
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) site_url: Option<String>,
}

impl FeedMetadata {
    /// Returns true when there is at least one non-empty metadata field to persist.
    pub(crate) fn has_values(&self) -> bool {
        self.title.is_some() || self.author.is_some() || self.site_url.is_some()
    }
}

/// Normalized entry with tags and content payload.
#[derive(Debug)]
pub(crate) struct SyncEntry {
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

#[cfg(test)]
mod tests {
    use super::SyncStatus;

    #[test]
    fn sync_status_marks_non_completed_states_as_warning() {
        assert!(!SyncStatus::Completed.is_warning());
        assert!(SyncStatus::PartialFailed.is_warning());
        assert!(SyncStatus::Failed.is_warning());
    }
}
