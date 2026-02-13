//! Common response envelope for CLI output.

use crate::error::{AppError, ErrorPayload};
use crate::time;
use schemars::JsonSchema;
use serde::Serialize;

/// Severity level of the response.
#[derive(Debug, Serialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ResponseSeverity {
    /// Command completed without warnings.
    Ok,
    /// Command completed with non-fatal warnings.
    Warn,
    /// Command failed fatally.
    Error,
}

/// Metadata returned on every JSON response.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ResponseMeta {
    /// Current CLI API version.
    pub api_version: &'static str,
    /// Current database schema version.
    pub schema_version: i64,
    /// Epoch seconds when the response was generated.
    pub generated_at: i64,
}

impl ResponseMeta {
    /// Builds metadata using current process build/runtime values.
    pub fn current() -> Self {
        Self {
            api_version: env!("CARGO_PKG_VERSION"),
            schema_version: crate::db::migrate::current_schema_version(),
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
    /// Whether the command succeeded.
    pub success: bool,
    /// Severity of the command outcome.
    pub severity: ResponseSeverity,
    /// Command payload when `success=true`, otherwise `null`.
    pub result: Option<T>,
    /// Error payload when `success=false`, otherwise `null`.
    pub error: Option<ErrorPayload>,
    /// Response metadata.
    pub meta: ResponseMeta,
}

impl<T> Envelope<T> {
    /// Builds a success envelope with payload and explicit severity.
    pub fn ok_with_severity(data: T, severity: ResponseSeverity) -> Self {
        Self {
            success: true,
            severity,
            result: Some(data),
            error: None,
            meta: ResponseMeta::current(),
        }
    }

    /// Builds a fatal error envelope from an [`AppError`].
    pub fn fatal(error: &AppError) -> Self {
        Self {
            success: false,
            severity: ResponseSeverity::Error,
            result: None,
            error: Some(ErrorPayload::from_error(error)),
            meta: ResponseMeta::current(),
        }
    }
}
