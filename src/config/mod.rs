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
    /// Sync configuration.
    pub sync: SyncConfig,
    /// Content storage configuration.
    pub storage: StorageConfig,
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

/// Sync configuration.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Number of parallel fetch workers.
    pub parallel: usize,
    /// HTTP timeout in seconds.
    pub timeout_secs: u64,
    /// HTTP user agent.
    pub user_agent: String,
    /// Retry count for fetch failures.
    pub retry_count: u32,
    /// Retry delay in seconds.
    pub retry_delay_secs: u64,
}

/// Content storage configuration.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Storage mode for entry contents.
    pub content_store: ContentStore,
    /// Root directory for file storage.
    pub data_dir: PathBuf,
}

/// Content storage mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentStore {
    /// Store content in SQLite.
    Db,
    /// Store content in filesystem.
    Fs,
    /// Do not store content.
    None,
}

/// Raw config.toml representation.
#[derive(Debug, Deserialize)]
struct AppConfigRaw {
    unread_tag: Option<String>,
    database: DatabaseConfigRaw,
    feeds: FeedsSourceConfigRaw,
    sync: Option<SyncConfigRaw>,
    storage: Option<StorageConfigRaw>,
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

/// Raw sync config representation.
#[derive(Debug, Deserialize)]
struct SyncConfigRaw {
    parallel: Option<usize>,
    timeout: Option<u64>,
    user_agent: Option<String>,
    retry_count: Option<u32>,
    retry_delay: Option<u64>,
}

/// Raw storage config representation.
#[derive(Debug, Deserialize)]
struct StorageConfigRaw {
    content_store: Option<String>,
    data_dir: Option<String>,
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
        let sync = SyncConfig::from_raw(raw.sync)?;
        let storage = StorageConfig::from_raw(raw.storage)?;
        Ok(Self {
            unread_tag,
            database: DatabaseConfig {
                path: database_path,
            },
            feeds: FeedsSourceConfig { source: feeds_path },
            sync,
            storage,
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

impl SyncConfig {
    /// Builds a SyncConfig from optional raw config.
    fn from_raw(raw: Option<SyncConfigRaw>) -> Result<Self, AppError> {
        let raw = raw.unwrap_or(SyncConfigRaw {
            parallel: None,
            timeout: None,
            user_agent: None,
            retry_count: None,
            retry_delay: None,
        });
        Ok(Self {
            parallel: raw.parallel.unwrap_or(5).max(1),
            timeout_secs: raw.timeout.unwrap_or(30),
            user_agent: raw.user_agent.unwrap_or_else(|| "feeder/0.1.0".to_string()),
            retry_count: raw.retry_count.unwrap_or(3),
            retry_delay_secs: raw.retry_delay.unwrap_or(5),
        })
    }
}

impl StorageConfig {
    /// Builds a StorageConfig from optional raw config.
    fn from_raw(raw: Option<StorageConfigRaw>) -> Result<Self, AppError> {
        let raw = raw.unwrap_or(StorageConfigRaw {
            content_store: None,
            data_dir: None,
        });
        let store = parse_content_store(raw.content_store.as_deref())?;
        let data_dir = match raw.data_dir {
            Some(path) => expand_path(&path)?,
            None => expand_path("~/.local/share/feeder/data")?,
        };
        Ok(Self {
            content_store: store,
            data_dir,
        })
    }
}

/// Parses the content_store value into ContentStore.
fn parse_content_store(value: Option<&str>) -> Result<ContentStore, AppError> {
    match value.unwrap_or("db") {
        "db" => Ok(ContentStore::Db),
        "fs" => Ok(ContentStore::Fs),
        "none" => Ok(ContentStore::None),
        other => Err(AppError::config(format!(
            "Invalid storage.content_store value: {other}"
        ))),
    }
}
