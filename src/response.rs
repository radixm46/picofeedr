//! Common response envelope for CLI output.

use crate::error::{AppError, ErrorPayload};
use crate::time;
use crate::{
    config::feeds::ConfigCheckReport, entry::EntryDetail, entry::EntryListResponse,
    feed::FeedListResponse, status::StatusResponse, sync::SyncSummary,
};
use schemars::JsonSchema;
use serde::Serialize;

/// Status of the response.
#[derive(Debug, Serialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    /// Command completed without warnings.
    Ok,
    /// Command completed with non-fatal warnings.
    Warning,
    /// Command failed fatally.
    Error,
}

/// Metadata returned on every JSON response.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ResponseMeta {
    /// Current CLI API version.
    pub api_version: &'static str,
    /// Current database schema version.
    pub db_schema_version: i64,
    /// Epoch seconds when the response was generated.
    pub generated_at: i64,
}

impl ResponseMeta {
    /// Builds metadata using current process build/runtime values.
    pub fn current() -> Self {
        Self {
            api_version: env!("CARGO_PKG_VERSION"),
            db_schema_version: crate::db::migrate::current_schema_version(),
            generated_at: time::current_epoch(),
        }
    }
}

/// Version payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VersionResponse {
    /// CLI API version.
    pub api_version: String,
    /// SQLite schema version.
    pub db_schema_version: i64,
    /// Build channel label.
    pub build: String,
}

/// Tags payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TagListResponse {
    /// Known tag dictionary.
    pub tags: Vec<String>,
}

/// Mark payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkResponse {
    /// Number of entries updated by mark command.
    pub updated_entry_count: usize,
}

/// Trait for payloads that can be wrapped in the standard JSON response envelope.
pub trait ResponsePayload: Serialize + JsonSchema + Sized {
    /// Returns the envelope status for this payload.
    fn response_status(&self) -> ResponseStatus {
        ResponseStatus::Ok
    }

    /// Wraps the payload in the standard JSON response envelope.
    fn into_envelope(self) -> Envelope<Self> {
        let status = self.response_status();
        Envelope::ok_with_status(self, status)
    }
}

impl ResponsePayload for VersionResponse {}

impl ResponsePayload for TagListResponse {}

impl ResponsePayload for MarkResponse {}

impl ResponsePayload for StatusResponse {}

impl ResponsePayload for FeedListResponse {}

impl ResponsePayload for EntryListResponse {}

impl ResponsePayload for EntryDetail {}

impl ResponsePayload for SyncSummary {
    fn response_status(&self) -> ResponseStatus {
        if self.status.is_warning() {
            ResponseStatus::Warning
        } else {
            ResponseStatus::Ok
        }
    }
}

impl ResponsePayload for ConfigCheckReport {
    fn response_status(&self) -> ResponseStatus {
        if self.has_errors() {
            ResponseStatus::Warning
        } else {
            ResponseStatus::Ok
        }
    }
}

/// CLI response envelope for `--output json`.
///
/// This format is stable and intended for UI/automation clients.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(bound = "T: JsonSchema")]
pub struct Envelope<T> {
    /// Status of the command outcome.
    status: ResponseStatus,
    /// Command payload when `status` is `ok` or `warning`, otherwise `null`.
    result: Option<T>,
    /// Error payload when `status=error`, otherwise `null`.
    error: Option<ErrorPayload>,
    /// Response metadata.
    meta: ResponseMeta,
}

impl<T> Envelope<T> {
    /// Builds a success envelope with payload and explicit status.
    pub fn ok_with_status(data: T, status: ResponseStatus) -> Self {
        debug_assert!(matches!(
            status,
            ResponseStatus::Ok | ResponseStatus::Warning
        ));
        Self {
            status,
            result: Some(data),
            error: None,
            meta: ResponseMeta::current(),
        }
    }

    /// Builds a success envelope with `status=ok`.
    pub fn ok(data: T) -> Self {
        Self::ok_with_status(data, ResponseStatus::Ok)
    }

    /// Builds a warning envelope with payload and `status=warning`.
    pub fn warning(data: T) -> Self {
        Self::ok_with_status(data, ResponseStatus::Warning)
    }

    /// Builds a fatal error envelope from an [`AppError`].
    pub fn fatal(error: &AppError) -> Self {
        Self {
            status: ResponseStatus::Error,
            result: None,
            error: Some(ErrorPayload::from_error(error)),
            meta: ResponseMeta::current(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MarkResponse, ResponsePayload, TagListResponse, VersionResponse};
    use crate::config::feeds::ConfigCheckReport;
    use crate::sync::{SyncStatus, SyncSummary};

    #[test]
    fn version_response_serializes_expected_keys() {
        let value = serde_json::to_value(VersionResponse {
            api_version: "1.2.3".to_string(),
            db_schema_version: 7,
            build: "dev".to_string(),
        })
        .expect("serialize version");
        assert_eq!(
            value,
            serde_json::json!({
                "api_version": "1.2.3",
                "db_schema_version": 7,
                "build": "dev",
            })
        );
    }

    #[test]
    fn tag_list_and_mark_payloads_serialize_stably() {
        let tags = serde_json::to_value(TagListResponse {
            tags: vec!["rust".to_string(), "tech".to_string()],
        })
        .expect("serialize tags");
        assert_eq!(tags, serde_json::json!({ "tags": ["rust", "tech"] }));

        let mark = serde_json::to_value(MarkResponse {
            updated_entry_count: 2,
        })
        .expect("serialize mark");
        assert_eq!(mark, serde_json::json!({ "updated_entry_count": 2 }));
    }

    #[test]
    fn sync_summary_into_envelope_uses_warning_for_non_completed_status() {
        let value = serde_json::to_value(
            SyncSummary {
                status: SyncStatus::PartialFailed,
                fetched_feed_count: 2,
                failed_feed_count: 1,
                new_entry_count: 3,
                duration_ms: 10,
                errors: Vec::new(),
            }
            .into_envelope(),
        )
        .expect("serialize sync envelope");
        assert_eq!(value["status"], "warning");
        assert_eq!(value["result"]["status"], "partial_failed");
        assert!(value["error"].is_null());
    }

    #[test]
    fn config_check_report_into_envelope_uses_warning_for_invalid_report() {
        let value = serde_json::to_value(
            ConfigCheckReport {
                valid: false,
                errors: Vec::new(),
                warnings: Vec::new(),
                checked_feeds: 1,
            }
            .into_envelope(),
        )
        .expect("serialize config check envelope");
        assert_eq!(value["status"], "warning");
        assert_eq!(value["result"]["valid"], false);
        assert!(value["error"].is_null());
    }
}
