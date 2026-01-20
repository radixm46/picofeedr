//! CLI integration tests.

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use tempfile::TempDir;

/// Ensures feeds --config-check reports config-only feeds.
#[test]
fn feeds_config_check_reports_new_feeds() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let output = cargo_bin_cmd!("feeder")
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("feeds")
        .arg("--config-check")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: Value = serde_json::from_slice(&output).expect("json");
    let new_in_config = value
        .get("new_in_config")
        .and_then(|value| value.as_array())
        .expect("new_in_config array");
    assert_eq!(new_in_config.len(), 1);
    assert_eq!(new_in_config[0]["url"], "https://example.com/feed");
}

/// Ensures feeds command reconciles and returns tags.
#[test]
fn feeds_reconcile_returns_tags() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let output = cargo_bin_cmd!("feeder")
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("feeds")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: Value = serde_json::from_slice(&output).expect("json");
    let feeds = value
        .get("feeds")
        .and_then(|value| value.as_array())
        .expect("feeds array");
    assert_eq!(feeds.len(), 1);
    let tags = feeds[0]["tags"].as_array().expect("tags array");
    let tags: Vec<String> = tags
        .iter()
        .map(|tag| tag.as_str().unwrap().to_string())
        .collect();
    assert_eq!(tags, vec!["tech", "rust"]);
}

/// Ensures tags command lists config-derived tags after reconciliation.
#[test]
fn tags_command_returns_tag_dictionary() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    cargo_bin_cmd!("feeder")
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("feeds")
        .assert()
        .success();

    let output = cargo_bin_cmd!("feeder")
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("tags")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: Value = serde_json::from_slice(&output).expect("json");
    let tags = value
        .get("tags")
        .and_then(|value| value.as_array())
        .expect("tags array");
    let tag_values: Vec<String> = tags
        .iter()
        .map(|tag| tag.as_str().unwrap().to_string())
        .collect();
    assert_eq!(tag_values, vec!["rust", "tech", "unread"]);
}

/// Fixture file paths for CLI tests.
struct FixturePaths {
    config_path: String,
    db_path: String,
}

/// Writes config.toml and feeds.yaml fixtures.
fn write_fixture_files(temp: &TempDir) -> FixturePaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");

    let config = format!(
        r#"unread_tag = "unread"

[database]
path = "{}"

[feeds]
source = "{}"
"#,
        db_path.display(),
        feeds_path.display()
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
    }
}
