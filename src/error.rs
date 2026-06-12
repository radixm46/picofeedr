//! Application error types and JSON responses.

use rusqlite::Error as SqlError;
use rusqlite::ErrorCode as SqlErrorCode;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value};
use std::error::Error as StdError;
use thiserror::Error;

type BoxError = Box<dyn StdError + Send + Sync>;
/// Machine-readable error details object.
pub type ErrorDetails = Map<String, Value>;

/// Builds a details object map from key/value entries.
pub fn error_details<I, K>(entries: I) -> ErrorDetails
where
    I: IntoIterator<Item = (K, Value)>,
    K: Into<String>,
{
    entries.into_iter().map(|(k, v)| (k.into(), v)).collect()
}

/// Error codes exposed by the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[error("{message}")]
pub struct AppError {
    /// Error code.
    code: ErrorCode,
    /// Human-readable message.
    message: String,
    /// Optional machine-readable details.
    details: Option<ErrorDetails>,
    /// Source error when available.
    #[source]
    source: Option<BoxError>,
}

impl AppError {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            source: None,
        }
    }

    fn with_details(code: ErrorCode, message: impl Into<String>, details: ErrorDetails) -> Self {
        Self {
            details: Some(details),
            ..Self::new(code, message)
        }
    }

    fn with_source(
        code: ErrorCode,
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self {
            source: Some(Box::new(source)),
            ..Self::new(code, message)
        }
    }

    fn with_details_and_source(
        code: ErrorCode,
        message: impl Into<String>,
        details: ErrorDetails,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self {
            details: Some(details),
            source: Some(Box::new(source)),
            ..Self::new(code, message)
        }
    }

    /// Returns the error code.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// Returns the error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns whether the operation is safe to retry.
    pub fn retry(&self) -> bool {
        self.code == ErrorCode::DbLocked
    }

    /// Creates a configuration error.
    pub fn config(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ConfigError, message)
    }

    /// Creates a configuration error with details.
    pub fn config_with_details(message: impl Into<String>, details: ErrorDetails) -> Self {
        Self::with_details(ErrorCode::ConfigError, message, details)
    }

    /// Creates a configuration error with source.
    pub fn config_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::with_source(ErrorCode::ConfigError, message, source)
    }

    /// Creates an invalid query error.
    pub fn invalid_query(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidQuery, message)
    }

    /// Creates an invalid query error with details.
    pub fn invalid_query_with_details(message: impl Into<String>, details: ErrorDetails) -> Self {
        Self::with_details(ErrorCode::InvalidQuery, message, details)
    }

    /// Creates an entry not found error with details.
    pub fn entry_not_found_with_details(message: impl Into<String>, details: ErrorDetails) -> Self {
        Self::with_details(ErrorCode::EntryNotFound, message, details)
    }

    /// Creates an entry not found error.
    pub fn entry_not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::EntryNotFound, message)
    }

    /// Creates an I/O error.
    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::IoError, message)
    }

    /// Creates an I/O error with details.
    pub fn io_with_details(message: impl Into<String>, details: ErrorDetails) -> Self {
        Self::with_details(ErrorCode::IoError, message, details)
    }

    /// Creates an I/O error with source.
    pub fn io_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::with_source(ErrorCode::IoError, message, source)
    }

    /// Creates an I/O error with details and source.
    pub fn io_with_details_and_source(
        message: impl Into<String>,
        details: ErrorDetails,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::with_details_and_source(ErrorCode::IoError, message, details, source)
    }

    /// Creates a serialization error with source.
    pub fn serialization_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::with_source(ErrorCode::SerializationError, message, source)
    }

    /// Creates a database error.
    pub fn db(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::DbError, message)
    }

    /// Creates a database error with source.
    pub fn db_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::with_source(ErrorCode::DbError, message, source)
    }

    /// Creates a database error with details.
    pub fn db_with_details(message: impl Into<String>, details: ErrorDetails) -> Self {
        Self::with_details(ErrorCode::DbError, message, details)
    }

    /// Creates a locked database error with details and source.
    pub fn db_locked_with_details(
        message: impl Into<String>,
        details: ErrorDetails,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::with_details_and_source(ErrorCode::DbLocked, message, details, source)
    }

    /// Creates an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    /// Returns optional machine-readable error details.
    pub fn details(&self) -> Option<&ErrorDetails> {
        self.details.as_ref()
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
            let sqlite_code = match &error {
                SqlError::SqliteFailure(sql_error, _) => Some(format!("{:?}", sql_error.code)),
                _ => None,
            };
            AppError::db_locked_with_details(
                error.to_string(),
                error_details([
                    ("sqlite_code", sqlite_code.into()),
                    ("retry_after_ms", Value::from(200)),
                ]),
                error,
            )
        } else {
            AppError::db_with_source(error.to_string(), error)
        }
    }
}

/// JSON error payload fields.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorPayload {
    /// Error code string.
    #[schemars(regex(pattern = "^[A-Z][A-Z0-9_]*$"))]
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Whether the caller should retry.
    pub retryable: bool,
    /// Optional machine-readable details for error-specific branching.
    pub details: Option<ErrorDetails>,
}

impl ErrorPayload {
    /// Builds an error payload from an [`AppError`].
    pub fn from_error(error: &AppError) -> Self {
        Self {
            code: error.code().as_str().to_string(),
            message: error.message().to_string(),
            retryable: error.retry(),
            details: error.details().cloned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;
    use serde_json::{Map, Value};

    #[test]
    fn config_with_details_accepts_object_map() {
        let mut details = Map::new();
        details.insert(
            "hint".to_string(),
            Value::String("invalid_config".to_string()),
        );
        let error = AppError::config_with_details("invalid config", details.clone());
        let extracted = error.details().expect("details");
        assert_eq!(extracted, &details);
    }
}
