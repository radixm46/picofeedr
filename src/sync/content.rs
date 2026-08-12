//! Content selection and storage helpers.

use crate::config::{AppConfig, ContentStore};
use crate::content_ref;
use crate::db::{EntryContentInput, EntryContentStorage};
use crate::error::{AppError, error_details};
use crate::sync::model::EntryContentPlan;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;

/// Selects the best content payload from a feed entry.
pub(crate) fn select_content(entry: &feed_rs::model::Entry) -> (Option<String>, Option<String>) {
    if let Some(content) = &entry.content
        && let Some(body) = &content.body
    {
        return (Some(body.clone()), Some(content.content_type.to_string()));
    }
    if let Some(summary) = &entry.summary {
        return (
            Some(summary.content.clone()),
            Some(summary.content_type.to_string()),
        );
    }
    (None, None)
}

/// Builds entry content payload according to storage config.
pub(crate) fn build_entry_content(
    config: &AppConfig,
    content: Option<String>,
    content_type: Option<String>,
) -> EntryContentPlan {
    let Some(content) = content else {
        return EntryContentPlan {
            content: EntryContentInput {
                storage: EntryContentStorage::None,
                reference: None,
                content_type,
                content: None,
            },
            payload: None,
        };
    };
    match config.storage.content_store {
        ContentStore::Db => EntryContentPlan {
            content: EntryContentInput {
                storage: EntryContentStorage::Db,
                reference: None,
                content_type,
                content: Some(content),
            },
            payload: None,
        },
        ContentStore::Fs => {
            let reference = content_hash(&content);
            EntryContentPlan {
                content: EntryContentInput {
                    storage: EntryContentStorage::Fs,
                    reference: Some(reference),
                    content_type,
                    content: None,
                },
                payload: Some(content),
            }
        }
        ContentStore::None => EntryContentPlan {
            content: EntryContentInput {
                storage: EntryContentStorage::None,
                reference: None,
                content_type,
                content: None,
            },
            payload: None,
        },
    }
}

/// Computes the content hash for filesystem storage.
fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)
}

fn write_content_file<W: Write>(
    path: &Path,
    reference: &str,
    mut file: W,
    content: &str,
) -> Result<(), AppError> {
    file.write_all(content.as_bytes()).map_err(|error| {
        let _ = fs::remove_file(path);
        AppError::io_with_details_and_source(
            format!("Failed to write content file: {error}"),
            error_details([
                ("path", Value::from(path.to_string_lossy().to_string())),
                ("reference", Value::from(reference.to_string())),
                ("hint", Value::from("failed_to_write_content_file")),
            ]),
            error,
        )
    })?;
    Ok(())
}

/// Stores content on filesystem if missing and returns whether it was created.
pub(crate) fn write_content_fs(
    root: &Path,
    reference: &str,
    content: &str,
) -> Result<bool, AppError> {
    let path = content_ref::sha256_path(root, reference)?;
    let dir = path
        .parent()
        .ok_or_else(|| AppError::internal("Invalid content path".to_string()))?;
    fs::create_dir_all(dir).map_err(|error| {
        AppError::io_with_details_and_source(
            format!("Failed to create content directory: {error}"),
            error_details([
                ("path", Value::from(dir.to_string_lossy().to_string())),
                ("reference", Value::from(reference.to_string())),
                ("hint", Value::from("failed_to_create_content_directory")),
            ]),
            error,
        )
    })?;
    let file = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => {
            return Err(AppError::io_with_details_and_source(
                format!("Failed to write content file: {error}"),
                error_details([
                    ("path", Value::from(path.to_string_lossy().to_string())),
                    ("reference", Value::from(reference.to_string())),
                    ("hint", Value::from("failed_to_write_content_file")),
                ]),
                error,
            ));
        }
    };
    write_content_file(&path, reference, file, content)?;
    Ok(true)
}

/// Removes filesystem content if present.
pub(crate) fn remove_content_fs(root: &Path, reference: &str) -> Result<(), AppError> {
    let path = content_ref::sha256_path(root, reference)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::io(format!("Failed to remove content: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::{write_content_file, write_content_fs};
    use serde_json::Value;
    use std::fs;
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use tempfile::tempdir;

    struct PartialWriter {
        file: File,
    }

    impl Write for PartialWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let partial_len = bytes.len().min(2);
            self.file.write_all(&bytes[..partial_len])?;
            Err(io::Error::other("forced write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }

    #[test]
    fn write_content_fs_returns_contextual_error_when_directory_creation_fails() {
        let dir = tempdir().expect("tempdir");
        let blocked_root = dir.path().join("blocked-root");
        fs::write(&blocked_root, "not a directory").expect("write blocker file");
        let reference = "a".repeat(64);

        let error = write_content_fs(&blocked_root, &reference, "hello").expect_err("write fails");

        assert_eq!(error.code().as_str(), "IO_ERROR");
        let details = error.details().expect("details");
        assert_eq!(details["hint"], "failed_to_create_content_directory");
        assert!(
            details
                .get("reference")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        );
        assert!(
            details
                .get("path")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        );
    }

    #[test]
    fn write_content_file_removes_partial_file_after_write_failure() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("content");
        let reference = "b".repeat(64);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("content file");

        let error = write_content_file(&path, &reference, PartialWriter { file }, "payload")
            .expect_err("write fails");

        assert_eq!(error.code().as_str(), "IO_ERROR");
        assert!(error.to_string().contains("forced write failure"));
        assert!(!path.exists());
    }
}
