//! Common response envelope for CLI output.

use crate::error::{AppError, ErrorPayload};
use serde::Serialize;

/// CLI response envelope for `--output json`.
///
/// This format is stable and intended for UI/automation clients.
#[derive(Debug, Serialize)]
pub struct Envelope<T> {
    /// Whether the command succeeded.
    pub ok: bool,
    /// Command payload when `ok=true`, otherwise `null`.
    pub data: Option<T>,
    /// Error payload when `ok=false`, otherwise `null`.
    pub error: Option<ErrorPayload>,
}

impl<T> Envelope<T> {
    /// Builds a success envelope with payload.
    pub fn ok(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    /// Builds a fatal error envelope from an [`AppError`].
    pub fn fatal(error: &AppError) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(ErrorPayload::from_error(error)),
        }
    }
}
