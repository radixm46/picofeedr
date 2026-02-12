//! Fixture builders and helpers for CLI integration tests.

use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const DEFAULT_SYNC_XML: &str = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Example Feed</title>
    <link>https://example.com</link>
    <description>Example Feed</description>
    <item>
      <title>First Entry</title>
      <link>https://example.com/1</link>
      <guid>entry-1</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <description>Hello world</description>
    </item>
    <item>
      <title>Second Entry</title>
      <link>https://example.com/2</link>
      <guid>entry-2</guid>
      <pubDate>Tue, 02 Jan 2024 00:00:00 GMT</pubDate>
      <description>Another entry</description>
    </item>
  </channel>
</rss>
"#;

const FS_SYNC_XML: &str = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Example Feed</title>
    <link>https://example.com</link>
    <description>Example Feed</description>
    <item>
      <title>First Entry</title>
      <link>https://example.com/1</link>
      <guid>entry-1</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <description>Hello world</description>
    </item>
  </channel>
</rss>
"#;

const OK_FEED_XML: &str = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>OK Feed</title>
    <link>https://example.com</link>
    <description>OK Feed</description>
    <item>
      <title>OK Entry</title>
      <link>https://example.com/ok</link>
      <guid>ok-entry</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <description>OK</description>
    </item>
  </channel>
</rss>
"#;

/// Fixture file paths for basic CLI tests.
pub struct FixturePaths {
    /// Path to generated config TOML.
    pub config_path: String,
    /// Path to generated database file.
    pub db_path: String,
    /// Path to generated feeds YAML.
    pub feeds_path: String,
}

/// Fixture file paths for sync tests.
pub struct SyncFixturePaths {
    /// Path to generated config TOML.
    pub config_path: String,
    /// Path to generated database file.
    pub db_path: String,
}

/// Fixture file paths for filesystem-content sync tests.
pub struct SyncFixtureFsPaths {
    /// Path to generated config TOML.
    pub config_path: String,
    /// Path to generated database file.
    pub db_path: String,
    /// Path to content storage directory.
    pub data_dir: String,
}

/// Guard that holds an exclusive SQLite lock until dropped.
pub struct ExclusiveDbLock {
    _conn: Connection,
}

/// Builder for standard sync fixtures.
pub struct SyncFixtureBuilder {
    root: PathBuf,
    unread_tag: String,
    default_limit: usize,
    max_limit: usize,
    content_store: ContentStore,
}

enum ContentStore {
    Db,
    Fs,
}

impl SyncFixtureBuilder {
    /// Creates a builder with default sync fixture settings.
    pub fn new(temp: &TempDir) -> Self {
        Self {
            root: temp.path().to_path_buf(),
            unread_tag: "unread".to_string(),
            default_limit: 100,
            max_limit: 1000,
            content_store: ContentStore::Db,
        }
    }

    /// Sets the unread tag used by query parsing.
    pub fn unread_tag(mut self, unread_tag: &str) -> Self {
        self.unread_tag = unread_tag.to_string();
        self
    }

    /// Sets query limits used by config.
    pub fn query_limits(mut self, default_limit: usize, max_limit: usize) -> Self {
        self.default_limit = default_limit;
        self.max_limit = max_limit;
        self
    }

    /// Stores entry content in the database.
    pub fn content_store_db(mut self) -> Self {
        self.content_store = ContentStore::Db;
        self
    }

    /// Stores entry content in the filesystem.
    pub fn content_store_fs(mut self) -> Self {
        self.content_store = ContentStore::Fs;
        self
    }

    /// Writes fixtures and returns paths for DB content storage.
    pub fn build_db(self) -> SyncFixturePaths {
        let root = self.root;
        let config_path = root.join("config.toml");
        let feeds_path = root.join("feeds.yaml");
        let db_path = root.join("db.sqlite");
        let feed_path = root.join("feed.xml");

        let config = render_sync_config(
            &db_path,
            &feeds_path,
            &self.unread_tag,
            self.default_limit,
            self.max_limit,
            &self.content_store,
        );

        let feed_url = format!("file://{}", feed_path.display());
        let feeds = format!(
            r#"feeds:
  tech:
    tags: [tech]
    feeds:
      - url: {feed_url}
        title: Example Feed
auto_tags:
  - title_contains: [First]
    add_tags: [hot]
    priority: 1
"#
        );

        fs::write(&config_path, config).expect("write config");
        fs::write(&feeds_path, feeds).expect("write feeds");
        let xml = match self.content_store {
            ContentStore::Db => DEFAULT_SYNC_XML,
            ContentStore::Fs => FS_SYNC_XML,
        };
        fs::write(&feed_path, xml).expect("write feed");

        SyncFixturePaths {
            config_path: config_path.display().to_string(),
            db_path: db_path.display().to_string(),
        }
    }

    /// Writes fixtures and returns paths for filesystem content storage.
    pub fn build_fs(self) -> SyncFixtureFsPaths {
        let root = self.root;
        let config_path = root.join("config.toml");
        let feeds_path = root.join("feeds.yaml");
        let db_path = root.join("db.sqlite");
        let feed_path = root.join("feed.xml");
        let data_dir = root.join("data");

        let config = render_sync_config(
            &db_path,
            &feeds_path,
            &self.unread_tag,
            self.default_limit,
            self.max_limit,
            &self.content_store,
        );

        let feed_url = format!("file://{}", feed_path.display());
        let feeds = format!(
            r#"feeds:
  tech:
    tags: [tech]
    feeds:
      - url: {feed_url}
        title: Example Feed
"#
        );

        fs::write(&config_path, config).expect("write config");
        fs::write(&feeds_path, feeds).expect("write feeds");
        fs::write(&feed_path, FS_SYNC_XML).expect("write feed");

        SyncFixtureFsPaths {
            config_path: config_path.display().to_string(),
            db_path: db_path.display().to_string(),
            data_dir: data_dir.display().to_string(),
        }
    }
}

/// Writes config TOML and feeds YAML for basic CLI fixtures.
pub fn write_fixture_files(temp: &TempDir) -> FixturePaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");

    let config = format!(
        r#"unread_tag = "unread"

[feeds]
source = "{}"

[storage]
root_dir = "{}"
"#,
        feeds_path.display(),
        temp.path().display()
    );

    let feeds = r#"feeds:
  tech:
    tags: [tech]
    rust:
      tags: [rust]
      feeds:
        - url: https://example.com/feed
          title: Example Feed
"#;

    fs::write(&config_path, config).expect("write config");
    fs::write(&feeds_path, feeds).expect("write feeds");

    FixturePaths {
        config_path: config_path.display().to_string(),
        db_path: db_path.display().to_string(),
        feeds_path: feeds_path.display().to_string(),
    }
}

/// Writes default sync fixture files with DB content storage.
pub fn write_sync_fixture_files(temp: &TempDir) -> SyncFixturePaths {
    SyncFixtureBuilder::new(temp).content_store_db().build_db()
}

/// Writes sync fixture files with custom unread tag.
pub fn write_sync_fixture_files_with_unread_tag(
    temp: &TempDir,
    unread_tag: &str,
) -> SyncFixturePaths {
    SyncFixtureBuilder::new(temp)
        .unread_tag(unread_tag)
        .content_store_db()
        .build_db()
}

/// Writes sync fixture files with custom query limits.
pub fn write_sync_fixture_files_with_query_limits(
    temp: &TempDir,
    unread_tag: &str,
    default_limit: usize,
    max_limit: usize,
) -> SyncFixturePaths {
    SyncFixtureBuilder::new(temp)
        .unread_tag(unread_tag)
        .query_limits(default_limit, max_limit)
        .content_store_db()
        .build_db()
}

/// Writes sync fixture files for filesystem content storage.
pub fn write_sync_fixture_files_fs(temp: &TempDir) -> SyncFixtureFsPaths {
    SyncFixtureBuilder::new(temp).content_store_fs().build_fs()
}

/// Writes fixture files for partial sync failure tests.
pub fn write_sync_failure_fixture_files(temp: &TempDir) -> SyncFixturePaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    let feed_ok_path = temp.path().join("feed_ok.xml");
    let feed_bad_path = temp.path().join("feed_bad.xml");

    let config = render_sync_config(
        &db_path,
        &feeds_path,
        "unread",
        100,
        1000,
        &ContentStore::Db,
    );

    let feed_ok_url = format!("file://{}", feed_ok_path.display());
    let feed_bad_url = format!("file://{}", feed_bad_path.display());
    let feeds = format!(
        r#"feeds:
  tech:
    tags: [tech]
    feeds:
      - url: {feed_ok_url}
        title: OK Feed
      - url: {feed_bad_url}
        title: Bad Feed
"#
    );

    fs::write(&config_path, config).expect("write config");
    fs::write(&feeds_path, feeds).expect("write feeds");
    fs::write(&feed_ok_path, OK_FEED_XML).expect("write ok feed");
    fs::write(&feed_bad_path, "not xml").expect("write bad feed");

    SyncFixturePaths {
        config_path: config_path.display().to_string(),
        db_path: db_path.display().to_string(),
    }
}

/// Writes fixture files for all-feed-failure sync tests.
pub fn write_sync_all_failed_fixture_files(temp: &TempDir) -> SyncFixturePaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    let feed_bad_path = temp.path().join("feed_bad.xml");

    let config = render_sync_config(
        &db_path,
        &feeds_path,
        "unread",
        100,
        1000,
        &ContentStore::Db,
    );

    let feed_bad_url = format!("file://{}", feed_bad_path.display());
    let feeds = format!(
        r#"feeds:
  tech:
    tags: [tech]
    feeds:
      - url: {feed_bad_url}
        title: Bad Feed
"#
    );

    fs::write(&config_path, config).expect("write config");
    fs::write(&feeds_path, feeds).expect("write feeds");
    fs::write(&feed_bad_path, "not xml").expect("write bad feed");

    SyncFixturePaths {
        config_path: config_path.display().to_string(),
        db_path: db_path.display().to_string(),
    }
}

/// Acquires an exclusive lock on the SQLite database until dropped.
pub fn acquire_exclusive_db_lock(db_path: &str) -> ExclusiveDbLock {
    let conn = Connection::open(db_path).expect("open db");
    conn.execute("BEGIN EXCLUSIVE", []).expect("lock db");
    ExclusiveDbLock { _conn: conn }
}

fn render_sync_config(
    db_path: &Path,
    feeds_path: &Path,
    unread_tag: &str,
    default_limit: usize,
    max_limit: usize,
    content_store: &ContentStore,
) -> String {
    let content_store = match content_store {
        ContentStore::Db => "db",
        ContentStore::Fs => "fs",
    };
    let root_dir = db_path
        .parent()
        .expect("db path should include a parent directory");

    format!(
        r#"unread_tag = "{unread_tag}"

[feeds]
source = "{}"

[sync]
parallel = 1
timeout = 5
user_agent = "picofeedr-test/0.1.0"
retry_count = 0
retry_delay = 0

[storage]
root_dir = "{}"
content_store = "{content_store}"

[query]
default_limit = {default_limit}
max_limit = {max_limit}
"#,
        feeds_path.display(),
        root_dir.display()
    )
}
