//! Common response envelope for CLI output.

use crate::error::{AppError, ErrorPayload};
use crate::time;
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

/// Ping status fixed literal.
#[derive(Debug, Serialize, JsonSchema)]
pub enum PingStatus {
    /// Fixed status string.
    #[serde(rename = "ok")]
    Ok,
}

/// Ping payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PingResult {
    /// Fixed heartbeat status.
    pub status: PingStatus,
}

impl PingResult {
    /// Builds a default ping payload.
    pub fn ok() -> Self {
        Self {
            status: PingStatus::Ok,
        }
    }
}

/// Version payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VersionResult {
    /// CLI API version.
    pub api_version: String,
    /// SQLite schema version.
    pub db_schema_version: i64,
    /// Build channel label.
    pub build: String,
}

/// Tags payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TagsResult {
    /// Known tag dictionary.
    pub tags: Vec<String>,
}

/// Mark payload.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkResult {
    /// Number of entries updated by mark command.
    pub updated_entry_count: usize,
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
    use super::{MarkResult, PingResult, TagsResult, VersionResult};

    #[test]
    fn ping_result_serializes_as_fixed_ok_status() {
        let value = serde_json::to_value(PingResult::ok()).expect("serialize ping");
        assert_eq!(value, serde_json::json!({ "status": "ok" }));
    }

    #[test]
    fn version_result_serializes_expected_keys() {
        let value = serde_json::to_value(VersionResult {
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
    fn tags_and_mark_payloads_serialize_stably() {
        let tags = serde_json::to_value(TagsResult {
            tags: vec!["rust".to_string(), "tech".to_string()],
        })
        .expect("serialize tags");
        assert_eq!(tags, serde_json::json!({ "tags": ["rust", "tech"] }));

        let mark = serde_json::to_value(MarkResult {
            updated_entry_count: 2,
        })
        .expect("serialize mark");
        assert_eq!(mark, serde_json::json!({ "updated_entry_count": 2 }));
    }
}
