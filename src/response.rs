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

/// CLI response envelope for `--output json`.
///
/// This format is stable and intended for UI/automation clients.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(bound = "T: JsonSchema")]
pub struct Envelope<T> {
    /// Status of the command outcome.
    pub status: ResponseStatus,
    /// Command payload when `status` is `ok` or `warning`, otherwise `null`.
    pub result: Option<T>,
    /// Error payload when `status=error`, otherwise `null`.
    pub error: Option<ErrorPayload>,
    /// Response metadata.
    pub meta: ResponseMeta,
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
