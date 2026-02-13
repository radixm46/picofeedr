//! Application error types and JSON responses.

use rusqlite::Error as SqlError;
use rusqlite::ErrorCode as SqlErrorCode;
use serde::Serialize;
use serde_json::Value;
use std::error::Error as StdError;
use thiserror::Error;

type BoxError = Box<dyn StdError + Send + Sync>;

/// Error codes exposed by the CLI.
#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    /// Configuration error.
    ConfigError,
    /// Invalid query syntax.
    InvalidQuery,
    /// Entry not found.
    EntryNotFound,
    /// Database is locked/busy and can be retried.
    DbLocked,
    /// Database error without retry.
    DbError,
    /// Internal unexpected error.
    Internal,
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
            ErrorCode::InvalidQuery => "INVALID_QUERY",
            ErrorCode::EntryNotFound => "ENTRY_NOT_FOUND",
            ErrorCode::DbLocked => "DB_LOCKED",
            ErrorCode::DbError => "DB_ERROR",
            ErrorCode::Internal => "INTERNAL",
            ErrorCode::IoError => "IO_ERROR",
            ErrorCode::SerializationError => "SERIALIZATION_ERROR",
        }
    }
}

/// Application error with code and retry flag.
#[derive(Debug, Error)]
pub enum AppError {
    /// Configuration error.
    #[error("{message}")]
    Config {
        /// Human-readable message.
        message: String,
        /// Source error when available.
        #[source]
        source: Option<BoxError>,
    },
    /// Query syntax error.
    #[error("{message}")]
    InvalidQuery {
        /// Human-readable message.
        message: String,
        /// Source error when available.
        #[source]
        source: Option<BoxError>,
    },
    /// Entry not found error.
    #[error("{message}")]
    EntryNotFound {
        /// Human-readable message.
        message: String,
        /// Source error when available.
        #[source]
        source: Option<BoxError>,
    },
    /// Database locked/busy error.
    #[error("{message}")]
    DbLocked {
        /// Human-readable message.
        message: String,
        /// Source error when available.
        #[source]
        source: Option<BoxError>,
    },
    /// Database error.
    #[error("{message}")]
    Db {
        /// Human-readable message.
        message: String,
        /// Source error when available.
        #[source]
        source: Option<BoxError>,
    },
    /// Internal unexpected error.
    #[allow(dead_code)]
    #[error("{message}")]
    Internal {
        /// Human-readable message.
        message: String,
        /// Source error when available.
        #[source]
        source: Option<BoxError>,
    },
    /// I/O error.
    #[error("{message}")]
    Io {
        /// Human-readable message.
        message: String,
        /// Source error when available.
        #[source]
        source: Option<BoxError>,
    },
    /// Serialization error.
    #[error("{message}")]
    Serialization {
        /// Human-readable message.
        message: String,
        /// Source error when available.
        #[source]
        source: Option<BoxError>,
    },
}

impl AppError {
    /// Returns the error code.
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::Config { .. } => ErrorCode::ConfigError,
            AppError::InvalidQuery { .. } => ErrorCode::InvalidQuery,
            AppError::EntryNotFound { .. } => ErrorCode::EntryNotFound,
            AppError::DbLocked { .. } => ErrorCode::DbLocked,
            AppError::Db { .. } => ErrorCode::DbError,
            AppError::Internal { .. } => ErrorCode::Internal,
            AppError::Io { .. } => ErrorCode::IoError,
            AppError::Serialization { .. } => ErrorCode::SerializationError,
        }
    }

    /// Returns the error message.
    pub fn message(&self) -> &str {
        match self {
            AppError::Config { message, .. }
            | AppError::InvalidQuery { message, .. }
            | AppError::EntryNotFound { message, .. }
            | AppError::DbLocked { message, .. }
            | AppError::Db { message, .. }
            | AppError::Internal { message, .. }
            | AppError::Io { message, .. }
            | AppError::Serialization { message, .. } => message,
        }
    }

    /// Returns whether the operation is safe to retry.
    pub fn retry(&self) -> bool {
        matches!(self, AppError::DbLocked { .. })
    }

    /// Creates a configuration error.
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
            source: None,
        }
    }

    /// Creates a configuration error with source.
    pub fn config_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::Config {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Creates an invalid query error.
    pub fn invalid_query(message: impl Into<String>) -> Self {
        Self::InvalidQuery {
            message: message.into(),
            source: None,
        }
    }

    /// Creates an entry not found error.
    pub fn entry_not_found(message: impl Into<String>) -> Self {
        Self::EntryNotFound {
            message: message.into(),
            source: None,
        }
    }

    /// Creates an I/O error.
    pub fn io(message: impl Into<String>) -> Self {
        Self::Io {
            message: message.into(),
            source: None,
        }
    }

    /// Creates an I/O error with source.
    pub fn io_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::Io {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Creates a serialization error with source.
    pub fn serialization_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::Serialization {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Creates a database error.
    pub fn db(message: impl Into<String>) -> Self {
        Self::Db {
            message: message.into(),
            source: None,
        }
    }

    /// Creates a database error with source.
    pub fn db_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::Db {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Creates a locked database error with source.
    pub fn db_locked_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::DbLocked {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Creates an internal error.
    #[allow(dead_code)]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
            source: None,
        }
    }
}

impl From<std::io::Error> for AppError {
    /// Converts I/O errors into AppError.
    fn from(error: std::io::Error) -> Self {
        AppError::io_with_source(error.to_string(), error)
    }
}

impl From<serde_json::Error> for AppError {
    /// Converts JSON errors into AppError.
    fn from(error: serde_json::Error) -> Self {
        AppError::serialization_with_source(error.to_string(), error)
    }
}

impl From<toml::de::Error> for AppError {
    /// Converts TOML errors into AppError.
    fn from(error: toml::de::Error) -> Self {
        AppError::config_with_source(error.to_string(), error)
    }
}

impl From<serde_yaml_ng::Error> for AppError {
    /// Converts YAML errors into AppError.
    fn from(error: serde_yaml_ng::Error) -> Self {
        AppError::config_with_source(error.to_string(), error)
    }
}

impl From<SqlError> for AppError {
    /// Converts SQLite errors into AppError, mapping locked/busy states.
    fn from(error: SqlError) -> Self {
        let is_locked = matches!(
            &error,
            SqlError::SqliteFailure(sql_error, _)
                if matches!(
                    sql_error.code,
                    SqlErrorCode::DatabaseBusy | SqlErrorCode::DatabaseLocked
                )
        );
        if is_locked {
            AppError::db_locked_with_source(error.to_string(), error)
        } else {
            AppError::db_with_source(error.to_string(), error)
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
    pub retriable: bool,
    /// Optional machine-readable details for error-specific branching.
    pub details: Option<Value>,
}

impl ErrorPayload {
    /// Builds an error payload from an [`AppError`].
    pub fn from_error(error: &AppError) -> Self {
        Self {
            code: error.code().as_str().to_string(),
            message: error.message().to_string(),
            retriable: error.retry(),
            details: None,
        }
    }
}
