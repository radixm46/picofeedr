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
    /// Query behavior configuration.
    pub query: QueryConfig,
    /// CLI configuration.
    pub cli: CliConfig,
    /// Logging configuration.
    pub log: LogConfig,
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
    /// Root directory that contains db.sqlite and data/.
    pub root_dir: PathBuf,
    /// Storage mode for entry contents.
    pub content_store: ContentStore,
    /// Root directory for file storage.
    pub data_dir: PathBuf,
}

/// Query configuration.
#[derive(Debug, Clone, Copy)]
pub struct QueryConfig {
    /// Default list limit used when --limit is not specified.
    pub default_limit: usize,
    /// Hard upper bound for list limit.
    pub max_limit: usize,
}

/// CLI configuration.
#[derive(Debug, Clone)]
pub struct CliConfig {
    /// Default output format.
    pub output: crate::cli::OutputFormat,
}

/// Logging configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogConfig {
    /// Log level for stderr diagnostics.
    pub level: LogLevel,
}

/// Log level definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
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
    feeds: FeedsSourceConfigRaw,
    sync: Option<SyncConfigRaw>,
    storage: StorageConfigRaw,
    query: Option<QueryConfigRaw>,
    cli: Option<CliConfigRaw>,
    log: Option<LogConfigRaw>,
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
    root_dir: String,
    content_store: Option<String>,
}

/// Raw query config representation.
#[derive(Debug, Deserialize)]
struct QueryConfigRaw {
    default_limit: Option<usize>,
    max_limit: Option<usize>,
}

/// Raw CLI config representation.
#[derive(Debug, Deserialize)]
struct CliConfigRaw {
    output: Option<String>,
}

/// Raw log config representation.
#[derive(Debug, Deserialize)]
struct LogConfigRaw {
    level: Option<String>,
}

impl AppConfig {
    /// Loads configuration from the default path or an override path.
    pub fn load(path_override: Option<PathBuf>) -> Result<Self, AppError> {
        let config_path = resolve_config_path(path_override)?;
        let content = fs::read_to_string(&config_path)
            .map_err(|error| AppError::config(format!("Failed to read config: {error}")))?;
        let raw: AppConfigRaw = toml::from_str(&content)?;
        let unread_tag = raw.unread_tag.unwrap_or_else(default_unread_tag);
        let feeds_path = expand_path(&raw.feeds.source)?;
        let sync = SyncConfig::from_raw(raw.sync)?;
        let storage = StorageConfig::from_raw(raw.storage)?;
        let query = QueryConfig::from_raw(raw.query)?;
        let cli = CliConfig::from_raw(raw.cli)?;
        let log = LogConfig::from_raw(raw.log)?;
        Ok(Self {
            unread_tag,
            database: DatabaseConfig {
                path: storage.root_dir.join("db.sqlite"),
            },
            feeds: FeedsSourceConfig { source: feeds_path },
            sync,
            storage,
            query,
            cli,
            log,
        })
    }

    /// Overrides the storage root directory from CLI arguments.
    pub fn override_root_dir(&mut self, path: PathBuf) -> Result<(), AppError> {
        let root_dir = expand_path(&path.to_string_lossy())?;
        self.storage.root_dir = root_dir;
        self.storage.data_dir = self.storage.root_dir.join("data");
        self.database.path = self.storage.root_dir.join("db.sqlite");
        Ok(())
    }
}

/// Resolves the config.toml path with a default fallback.
fn resolve_config_path(path_override: Option<PathBuf>) -> Result<PathBuf, AppError> {
    if let Some(path) = path_override {
        return expand_path(&path.to_string_lossy());
    }
    expand_path("~/.config/picofeedr/config.toml")
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
            user_agent: raw
                .user_agent
                .unwrap_or_else(|| "picofeedr/0.1.0".to_string()),
            retry_count: raw.retry_count.unwrap_or(3),
            retry_delay_secs: raw.retry_delay.unwrap_or(5),
        })
    }
}

impl StorageConfig {
    /// Builds a StorageConfig from raw config.
    fn from_raw(raw: StorageConfigRaw) -> Result<Self, AppError> {
        let store = parse_content_store(raw.content_store.as_deref())?;
        let root_dir = expand_path(&raw.root_dir)?;
        let data_dir = root_dir.join("data");
        Ok(Self {
            root_dir,
            content_store: store,
            data_dir,
        })
    }
}

impl CliConfig {
    /// Builds a CliConfig from optional raw config.
    fn from_raw(raw: Option<CliConfigRaw>) -> Result<Self, AppError> {
        let raw = raw.unwrap_or(CliConfigRaw { output: None });
        let output = parse_output_format(raw.output.as_deref())?;
        Ok(Self { output })
    }
}

impl QueryConfig {
    /// Builds a QueryConfig from optional raw config.
    fn from_raw(raw: Option<QueryConfigRaw>) -> Result<Self, AppError> {
        let raw = raw.unwrap_or(QueryConfigRaw {
            default_limit: None,
            max_limit: None,
        });
        let default_limit = raw.default_limit.unwrap_or(100);
        let max_limit = raw.max_limit.unwrap_or(1000);
        if default_limit == 0 {
            return Err(AppError::config(
                "query.default_limit must be greater than 0",
            ));
        }
        if max_limit == 0 {
            return Err(AppError::config("query.max_limit must be greater than 0"));
        }
        if default_limit > max_limit {
            return Err(AppError::config(
                "query.default_limit must be less than or equal to query.max_limit",
            ));
        }
        Ok(Self {
            default_limit,
            max_limit,
        })
    }
}

impl LogConfig {
    /// Builds a LogConfig from optional raw config.
    fn from_raw(raw: Option<LogConfigRaw>) -> Result<Self, AppError> {
        let raw = raw.unwrap_or(LogConfigRaw { level: None });
        let level = parse_log_level(raw.level.as_deref())?;
        Ok(Self { level })
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

/// Parses the CLI output format value.
fn parse_output_format(value: Option<&str>) -> Result<crate::cli::OutputFormat, AppError> {
    match value.unwrap_or("plain") {
        "json" => Ok(crate::cli::OutputFormat::Json),
        "plain" => Ok(crate::cli::OutputFormat::Plain),
        other => Err(AppError::config(format!(
            "Invalid cli.output value: {other}"
        ))),
    }
}

/// Parses the log level value.
fn parse_log_level(value: Option<&str>) -> Result<LogLevel, AppError> {
    match value.unwrap_or("info") {
        "error" => Ok(LogLevel::Error),
        "warn" => Ok(LogLevel::Warn),
        "info" => Ok(LogLevel::Info),
        "debug" => Ok(LogLevel::Debug),
        "trace" => Ok(LogLevel::Trace),
        other => Err(AppError::config(format!(
            "Invalid log.level value: {other}"
        ))),
    }
}
