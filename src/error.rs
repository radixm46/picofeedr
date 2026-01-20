//! Application error types and JSON responses.

use rusqlite::Error as SqlError;
use rusqlite::ErrorCode as SqlErrorCode;
use serde::Serialize;
use std::fmt::{Display, Formatter};

/// Error codes exposed by the CLI.
#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    /// Configuration error.
    ConfigError,
    /// Database is locked/busy and can be retried.
    DbLocked,
    /// Database error without retry.
    DbError,
    /// I/O error.
    IoError,
    /// Serialization error.
    SerializationError,
}

impl ErrorCode {
    /// Returns the string representation used in JSON output.
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::ConfigError => "CONFIG_ERROR",
            ErrorCode::DbLocked => "DB_LOCKED",
            ErrorCode::DbError => "DB_ERROR",
            ErrorCode::IoError => "IO_ERROR",
            ErrorCode::SerializationError => "SERIALIZATION_ERROR",
        }
    }
}

/// Application error with code and retry flag.
#[derive(Debug)]
pub struct AppError {
    /// Error code.
    pub code: ErrorCode,
    /// Human-readable message.
    pub message: String,
    /// Whether the operation is safe to retry.
    pub retry: bool,
}

impl AppError {
    /// Creates a configuration error.
    pub fn config(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::ConfigError,
            message: message.into(),
            retry: false,
        }
    }

    /// Creates an I/O error.
    pub fn io(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::IoError,
            message: message.into(),
            retry: false,
        }
    }

    /// Creates a serialization error.
    pub fn serialization(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::SerializationError,
            message: message.into(),
            retry: false,
        }
    }

    /// Creates a database error.
    pub fn db(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::DbError,
            message: message.into(),
            retry: false,
        }
    }

    /// Creates a locked database error.
    pub fn db_locked(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::DbLocked,
            message: message.into(),
            retry: true,
        }
    }
}

impl Display for AppError {
    /// Formats the error message.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    /// Converts I/O errors into AppError.
    fn from(error: std::io::Error) -> Self {
        AppError::io(error.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    /// Converts JSON errors into AppError.
    fn from(error: serde_json::Error) -> Self {
        AppError::serialization(error.to_string())
    }
}

impl From<toml::de::Error> for AppError {
    /// Converts TOML errors into AppError.
    fn from(error: toml::de::Error) -> Self {
        AppError::config(error.to_string())
    }
}

impl From<serde_yaml_ng::Error> for AppError {
    /// Converts YAML errors into AppError.
    fn from(error: serde_yaml_ng::Error) -> Self {
        AppError::config(error.to_string())
    }
}

impl From<SqlError> for AppError {
    /// Converts SQLite errors into AppError, mapping locked/busy states.
    fn from(error: SqlError) -> Self {
        match error {
            SqlError::SqliteFailure(ref error, _) => match error.code {
                SqlErrorCode::DatabaseBusy | SqlErrorCode::DatabaseLocked => {
                    AppError::db_locked(error.to_string())
                }
                _ => AppError::db(error.to_string()),
            },
            _ => AppError::db(error.to_string()),
        }
    }
}

/// JSON error response payload.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// Error object.
    pub error: ErrorPayload,
}

impl ErrorResponse {
    /// Builds an ErrorResponse from an AppError.
    pub fn from_error(error: &AppError) -> Self {
        Self {
            error: ErrorPayload {
                code: error.code.as_str().to_string(),
                message: error.message.clone(),
                retry: error.retry,
            },
        }
    }
}

/// JSON error payload fields.
#[derive(Debug, Serialize)]
pub struct ErrorPayload {
    /// Error code string.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Whether the caller should retry.
    pub retry: bool,
}
