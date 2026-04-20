//! Application configuration loader.

pub mod feeds;

use crate::error::{AppError, error_details};
use serde::Deserialize;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Application configuration derived from config.toml.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Whether unread management is enabled.
    pub manage_unread: bool,
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
    /// Maximum feed body size in bytes.
    pub max_feed_bytes: usize,
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
#[derive(Debug, Default, Deserialize)]
struct AppConfigRaw {
    manage_unread: Option<bool>,
    unread_tag: Option<String>,
    feeds: Option<FeedsSourceConfigRaw>,
    sync: Option<SyncConfigRaw>,
    storage: Option<StorageConfigRaw>,
    query: Option<QueryConfigRaw>,
    cli: Option<CliConfigRaw>,
    log: Option<LogConfigRaw>,
}

/// Raw feeds source config representation.
#[derive(Debug, Default, Deserialize)]
struct FeedsSourceConfigRaw {
    source: Option<String>,
}

/// Raw sync config representation.
#[derive(Debug, Deserialize)]
struct SyncConfigRaw {
    parallel: Option<usize>,
    timeout: Option<u64>,
    max_feed_bytes: Option<usize>,
    user_agent: Option<String>,
    retry_count: Option<u32>,
    retry_delay: Option<u64>,
}

/// Raw storage config representation.
#[derive(Debug, Default, Deserialize)]
struct StorageConfigRaw {
    root_dir: Option<String>,
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
        let config_path = resolve_config_path(path_override.clone())?;
        let raw = load_raw_config(&config_path, path_override.is_some())?;
        let manage_unread = raw.manage_unread.unwrap_or(default_manage_unread());
        let unread_tag = normalize_unread_tag(raw.unread_tag)?;
        let feeds = FeedsSourceConfig::from_raw(raw.feeds)?;
        let sync = SyncConfig::from_raw(raw.sync)?;
        let storage = StorageConfig::from_raw(raw.storage)?;
        let query = QueryConfig::from_raw(raw.query)?;
        let cli = CliConfig::from_raw(raw.cli)?;
        let log = LogConfig::from_raw(raw.log)?;
        Ok(Self {
            manage_unread,
            unread_tag,
            database: DatabaseConfig {
                path: storage.root_dir.join("db.sqlite"),
            },
            feeds,
            sync,
            storage,
            query,
            cli,
            log,
        })
    }

    /// Overrides the storage root directory from CLI arguments.
    pub fn override_root_dir(&mut self, path: PathBuf) -> Result<(), AppError> {
        let root_dir = expand_path_buf(path);
        self.storage.root_dir = root_dir;
        self.storage.data_dir = self.storage.root_dir.join("data");
        self.database.path = self.storage.root_dir.join("db.sqlite");
        Ok(())
    }

    /// Returns the configured unread tag name.
    pub fn unread_tag(&self) -> &str {
        self.unread_tag.as_str()
    }

    /// Returns the unread tag only when automatic unread assignment is enabled.
    pub fn auto_unread_tag(&self) -> Option<&str> {
        self.manage_unread.then_some(self.unread_tag.as_str())
    }
}

/// Resolves the config.toml path with a default fallback.
fn resolve_config_path(path_override: Option<PathBuf>) -> Result<PathBuf, AppError> {
    if let Some(path) = path_override {
        return Ok(expand_path_buf(path));
    }
    expand_path("~/.config/picofeedr/config.toml")
}

fn load_raw_config(config_path: &Path, explicit_path: bool) -> Result<AppConfigRaw, AppError> {
    let content = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(error) if !explicit_path && error.kind() == io::ErrorKind::NotFound => {
            return Ok(AppConfigRaw::default());
        }
        Err(error) => {
            return Err(AppError::config_with_details(
                format!("Failed to read config: {error}"),
                error_details([
                    (
                        "path",
                        Value::from(config_path.to_string_lossy().to_string()),
                    ),
                    ("hint", Value::from("failed_to_read_config")),
                ]),
            ));
        }
    };

    toml::from_str(&content).map_err(|error| {
        AppError::config_with_details(
            error.to_string(),
            error_details([
                (
                    "path",
                    Value::from(config_path.to_string_lossy().to_string()),
                ),
                ("hint", Value::from("invalid_toml")),
            ]),
        )
    })
}

/// Expands ~ in paths and returns a PathBuf.
fn expand_path(raw: &str) -> Result<PathBuf, AppError> {
    let expanded = shellexpand::tilde(raw);
    let path = Path::new(expanded.as_ref()).to_path_buf();
    Ok(path)
}

/// Expands a leading `~` component without converting the path to UTF-8.
fn expand_path_buf(path: PathBuf) -> PathBuf {
    expand_tilde_component(path.as_path(), current_home_dir())
}

fn expand_tilde_component(path: &Path, home_dir: Option<PathBuf>) -> PathBuf {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(component)) if component == OsStr::new("~") => {
            if let Some(home) = home_dir {
                let mut expanded = home;
                for component in components {
                    expanded.push(component.as_os_str());
                }
                expanded
            } else {
                path.to_path_buf()
            }
        }
        _ => path.to_path_buf(),
    }
}

fn current_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// Returns the default unread tag name.
fn default_unread_tag() -> &'static str {
    "unread"
}

fn default_manage_unread() -> bool {
    true
}

fn normalize_unread_tag(value: Option<String>) -> Result<String, AppError> {
    match value {
        None => Ok(default_unread_tag().to_string()),
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Err(AppError::config_with_details(
                    "unread_tag must not be empty",
                    error_details([
                        ("path", Value::from("unread_tag")),
                        ("hint", Value::from("set a non-empty unread_tag")),
                    ]),
                ))
            } else {
                Ok(trimmed.to_string())
            }
        }
    }
}

fn default_feeds_source() -> &'static str {
    "~/.config/picofeedr/feeds.yaml"
}

fn default_storage_root() -> &'static str {
    "~/.local/share/picofeedr"
}

impl FeedsSourceConfig {
    /// Builds a FeedsSourceConfig from optional raw config.
    fn from_raw(raw: Option<FeedsSourceConfigRaw>) -> Result<Self, AppError> {
        let raw = raw.unwrap_or_default();
        let source = expand_path(raw.source.as_deref().unwrap_or(default_feeds_source()))?;
        Ok(Self { source })
    }
}

impl SyncConfig {
    /// Builds a SyncConfig from optional raw config.
    fn from_raw(raw: Option<SyncConfigRaw>) -> Result<Self, AppError> {
        let raw = raw.unwrap_or(SyncConfigRaw {
            parallel: None,
            timeout: None,
            max_feed_bytes: None,
            user_agent: None,
            retry_count: None,
            retry_delay: None,
        });
        let max_feed_bytes = raw.max_feed_bytes.unwrap_or(2 * 1024 * 1024);
        if max_feed_bytes == 0 {
            return Err(AppError::config_with_details(
                "sync.max_feed_bytes must be greater than 0",
                error_details([
                    ("path", Value::from("sync.max_feed_bytes")),
                    (
                        "hint",
                        Value::from("set a positive integer number of bytes"),
                    ),
                ]),
            ));
        }
        Ok(Self {
            parallel: raw.parallel.unwrap_or(5).max(1),
            timeout_secs: raw.timeout.unwrap_or(30),
            max_feed_bytes,
            user_agent: raw
                .user_agent
                .unwrap_or_else(|| format!("picofeedr/{}", env!("CARGO_PKG_VERSION"))),
            retry_count: raw.retry_count.unwrap_or(3),
            retry_delay_secs: raw.retry_delay.unwrap_or(5),
        })
    }
}

impl StorageConfig {
    /// Builds a StorageConfig from raw config.
    fn from_raw(raw: Option<StorageConfigRaw>) -> Result<Self, AppError> {
        let raw = raw.unwrap_or_default();
        let store = parse_content_store(raw.content_store.as_deref())?;
        let root_dir = expand_path(raw.root_dir.as_deref().unwrap_or(default_storage_root()))?;
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
            return Err(AppError::config_with_details(
                "query.default_limit must be greater than 0",
                error_details([
                    ("path", Value::Null),
                    ("hint", Value::from("query.default_limit must be >= 1")),
                ]),
            ));
        }
        if max_limit == 0 {
            return Err(AppError::config_with_details(
                "query.max_limit must be greater than 0",
                error_details([
                    ("path", Value::Null),
                    ("hint", Value::from("query.max_limit must be >= 1")),
                ]),
            ));
        }
        if default_limit > max_limit {
            return Err(AppError::config_with_details(
                "query.default_limit must be less than or equal to query.max_limit",
                error_details([
                    ("path", Value::Null),
                    (
                        "hint",
                        Value::from("query.default_limit must be <= query.max_limit"),
                    ),
                ]),
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
        other => Err(AppError::config_with_details(
            format!("Invalid storage.content_store value: {other}"),
            error_details([
                ("path", Value::Null),
                ("hint", Value::from("allowed values: db|fs|none")),
            ]),
        )),
    }
}

/// Parses the CLI output format value.
fn parse_output_format(value: Option<&str>) -> Result<crate::cli::OutputFormat, AppError> {
    match value.unwrap_or("plain") {
        "json" => Ok(crate::cli::OutputFormat::Json),
        "plain" => Ok(crate::cli::OutputFormat::Plain),
        other => Err(AppError::config_with_details(
            format!("Invalid cli.output value: {other}"),
            error_details([
                ("path", Value::Null),
                ("hint", Value::from("allowed values: json|plain")),
            ]),
        )),
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
        other => Err(AppError::config_with_details(
            format!("Invalid log.level value: {other}"),
            error_details([
                ("path", Value::Null),
                (
                    "hint",
                    Value::from("allowed values: error|warn|info|debug|trace"),
                ),
            ]),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, CliConfig, ContentStore, DatabaseConfig, FeedsSourceConfig, LogConfig, LogLevel,
        QueryConfig, StorageConfig, SyncConfig, SyncConfigRaw, expand_tilde_component,
    };
    use std::path::PathBuf;

    fn test_app_config() -> AppConfig {
        AppConfig {
            manage_unread: true,
            unread_tag: "unread".to_string(),
            database: DatabaseConfig {
                path: PathBuf::from("/tmp/db.sqlite"),
            },
            feeds: FeedsSourceConfig {
                source: PathBuf::from("/tmp/feeds.yaml"),
            },
            sync: SyncConfig::from_raw(None).expect("sync defaults"),
            storage: StorageConfig {
                root_dir: PathBuf::from("/tmp/root"),
                content_store: ContentStore::Fs,
                data_dir: PathBuf::from("/tmp/root/data"),
            },
            query: QueryConfig {
                default_limit: 100,
                max_limit: 1000,
            },
            cli: CliConfig {
                output: crate::cli::OutputFormat::Plain,
            },
            log: LogConfig {
                level: LogLevel::Info,
            },
        }
    }

    #[test]
    fn default_sync_user_agent_uses_package_version() {
        let sync = SyncConfig::from_raw(None).expect("sync defaults");
        assert_eq!(
            sync.user_agent,
            format!("picofeedr/{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn default_sync_max_feed_bytes_is_two_mebibytes() {
        let sync = SyncConfig::from_raw(None).expect("sync defaults");
        assert_eq!(sync.max_feed_bytes, 2 * 1024 * 1024);
    }

    #[test]
    fn sync_max_feed_bytes_zero_is_invalid() {
        let error = SyncConfig::from_raw(Some(SyncConfigRaw {
            parallel: None,
            timeout: None,
            user_agent: None,
            retry_count: None,
            retry_delay: None,
            max_feed_bytes: Some(0),
        }))
        .expect_err("zero max_feed_bytes should fail");

        assert_eq!(error.code().as_str(), "CONFIG_ERROR");
        assert!(error.to_string().contains("sync.max_feed_bytes"));
    }

    #[test]
    fn expand_path_buf_expands_leading_tilde_component() {
        let home = PathBuf::from("/tmp/picofeedr-home");
        let expanded =
            expand_tilde_component(PathBuf::from("~/feeds").as_path(), Some(home.clone()));

        assert_eq!(expanded, home.join("feeds"));
    }

    #[cfg(unix)]
    #[test]
    fn override_root_dir_preserves_non_utf8_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let raw = OsString::from_vec(vec![0x66, 0x6f, 0x80, 0x6f]);
        let root_dir = PathBuf::from(&raw);
        let mut config = test_app_config();

        config
            .override_root_dir(root_dir.clone())
            .expect("override root dir");

        assert_eq!(config.storage.root_dir, root_dir);
        assert_eq!(config.storage.data_dir, PathBuf::from(&raw).join("data"));
        assert_eq!(config.database.path, PathBuf::from(&raw).join("db.sqlite"));
    }
}
