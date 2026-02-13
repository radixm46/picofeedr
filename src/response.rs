//! Common response envelope for CLI output.

use crate::error::{AppError, ErrorPayload};
use crate::time;
use serde::Serialize;

/// Metadata returned on every JSON response.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
pub struct Envelope<T> {
    /// Whether the command succeeded.
    pub success: bool,
    /// Command payload when `success=true`, otherwise `null`.
    pub result: Option<T>,
    /// Error payload when `success=false`, otherwise `null`.
    pub error: Option<ErrorPayload>,
    /// Response metadata.
    pub meta: ResponseMeta,
}

impl<T> Envelope<T> {
    /// Builds a success envelope with payload.
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            result: Some(data),
            error: None,
            meta: ResponseMeta::current(),
        }
    }

    /// Builds a fatal error envelope from an [`AppError`].
    pub fn fatal(error: &AppError) -> Self {
        Self {
            success: false,
            result: None,
            error: Some(ErrorPayload::from_error(error)),
            meta: ResponseMeta::current(),
        }
    }
}
