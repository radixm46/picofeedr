//! Application configuration loader.

pub mod feeds;

use crate::error::AppError;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Application configuration derived from config.toml.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Name of the unread tag.
    pub unread_tag: String,
    /// Database configuration.
    pub database: DatabaseConfig,
    /// Feeds configuration.
    pub feeds: FeedsSourceConfig,
}

/// Database configuration.
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// Path to SQLite database file.
    pub path: PathBuf,
}

/// Feeds configuration path.
#[derive(Debug, Clone)]
pub struct FeedsSourceConfig {
    /// Path to feeds.yaml file.
    pub source: PathBuf,
}

/// Raw config.toml representation.
#[derive(Debug, Deserialize)]
struct AppConfigRaw {
    unread_tag: Option<String>,
    database: DatabaseConfigRaw,
    feeds: FeedsSourceConfigRaw,
}

/// Raw database config representation.
#[derive(Debug, Deserialize)]
struct DatabaseConfigRaw {
    path: String,
}

/// Raw feeds source config representation.
#[derive(Debug, Deserialize)]
struct FeedsSourceConfigRaw {
    source: String,
}

impl AppConfig {
    /// Loads configuration from the default path or an override path.
    pub fn load(path_override: Option<PathBuf>) -> Result<Self, AppError> {
        let config_path = resolve_config_path(path_override)?;
        let content = fs::read_to_string(&config_path)
            .map_err(|error| AppError::config(format!("Failed to read config: {error}")))?;
        let raw: AppConfigRaw = toml::from_str(&content)?;
        let unread_tag = raw.unread_tag.unwrap_or_else(default_unread_tag);
        let database_path = expand_path(&raw.database.path)?;
        let feeds_path = expand_path(&raw.feeds.source)?;
        Ok(Self {
            unread_tag,
            database: DatabaseConfig {
                path: database_path,
            },
            feeds: FeedsSourceConfig { source: feeds_path },
        })
    }

    /// Overrides the database path from CLI arguments.
    pub fn override_db_path(&mut self, path: PathBuf) -> Result<(), AppError> {
        let expanded = expand_path(&path.to_string_lossy())?;
        self.database.path = expanded;
        Ok(())
    }
}

/// Resolves the config.toml path with a default fallback.
fn resolve_config_path(path_override: Option<PathBuf>) -> Result<PathBuf, AppError> {
    if let Some(path) = path_override {
        return expand_path(&path.to_string_lossy());
    }
    expand_path("~/.config/feeder/config.toml")
}

/// Expands ~ in paths and returns a PathBuf.
fn expand_path(raw: &str) -> Result<PathBuf, AppError> {
    let expanded = shellexpand::tilde(raw);
    let path = Path::new(expanded.as_ref()).to_path_buf();
    Ok(path)
}

/// Returns the default unread tag name.
fn default_unread_tag() -> String {
    "unread".to_string()
}
