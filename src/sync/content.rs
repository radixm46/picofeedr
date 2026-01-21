//! Content selection and storage helpers.

use crate::config::{AppConfig, ContentStore};
use crate::db::EntryContentInput;
use crate::error::AppError;
use sha2::{Digest, Sha256};
use std::fs;

/// Selects the best content payload from a feed entry.
pub(crate) fn select_content(entry: &feed_rs::model::Entry) -> (Option<String>, Option<String>) {
    if let Some(content) = &entry.content {
        if let Some(body) = &content.body {
            return (Some(body.clone()), Some(content.content_type.to_string()));
        }
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
) -> Result<Option<EntryContentInput>, AppError> {
    let Some(content) = content else {
        return Ok(Some(EntryContentInput {
            storage: "none".to_string(),
            reference: None,
            content_type,
            content: None,
        }));
    };
    match config.storage.content_store {
        ContentStore::Db => Ok(Some(EntryContentInput {
            storage: "db".to_string(),
            reference: None,
            content_type,
            content: Some(content),
        })),
        ContentStore::Fs => {
            let reference = store_content_fs(&config.storage.data_dir, &content)?;
            Ok(Some(EntryContentInput {
                storage: "fs".to_string(),
                reference: Some(reference),
                content_type,
                content: None,
            }))
        }
        ContentStore::None => Ok(Some(EntryContentInput {
            storage: "none".to_string(),
            reference: None,
            content_type,
            content: None,
        })),
    }
}

/// Stores content on filesystem and returns the hash reference.
fn store_content_fs(root: &std::path::Path, content: &str) -> Result<String, AppError> {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(digest);
    let (prefix, _) = hex.split_at(2);
    let dir = root.join(prefix);
    fs::create_dir_all(&dir)
        .map_err(|error| AppError::io(format!("Failed to create content dir: {error}")))?;
    let path = dir.join(&hex);
    fs::write(&path, content.as_bytes())
        .map_err(|error| AppError::io(format!("Failed to write content: {error}")))?;
    Ok(hex)
}
