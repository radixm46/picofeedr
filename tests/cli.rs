//! CLI integration tests.

use assert_cmd::cargo::cargo_bin_cmd;
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Creates a feeder command configured for JSON output.
fn feeder_cmd_json() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("feeder");
    cmd.arg("--output").arg("json");
    cmd
}

/// Creates a feeder command configured for plain output.
fn feeder_cmd_plain() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("feeder");
    cmd.arg("--output").arg("plain");
    cmd
}

/// Extracts the `data` object from a successful JSON envelope.
fn extract_ok_data(output: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(output).expect("json");
    assert_eq!(value["ok"], true, "expected ok=true envelope");
    value.get("data").cloned().expect("data")
}

/// Extracts error code from a failed JSON envelope.
fn extract_error_code(output: &[u8]) -> String {
    let value: Value = serde_json::from_slice(output).expect("json");
    assert_eq!(value["ok"], false, "expected ok=false envelope");
    value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(|code| code.as_str())
        .expect("error code")
        .to_string()
}

/// Extracts error payload from a failed JSON envelope.
fn extract_error_payload(output: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(output).expect("json");
    assert_eq!(value["ok"], false, "expected ok=false envelope");
    value.get("error").cloned().expect("error")
}

/// Ensures plain output is not JSON.
fn assert_plain_output(output: &[u8]) {
    let parsed = serde_json::from_slice::<Value>(output);
    assert!(parsed.is_err(), "expected plain (non-JSON) output");
}

/// Ensures feeds --config-check returns validation report fields.
#[test]
fn config_check_returns_validation_report() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let output = feeder_cmd_json()
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

    let data = extract_ok_data(&output);
    assert_eq!(data["valid"], true);
    assert!(data["errors"].as_array().expect("errors").is_empty());
    assert!(data["warnings"].as_array().expect("warnings").is_empty());
    assert_eq!(data["checked_feeds"], 1);
    assert!(data.get("new_in_config").is_none());
    assert!(data.get("removed_from_config").is_none());
    assert!(data.get("tag_changes").is_none());
}

/// Ensures duplicated feed URLs fail config check.
#[test]
fn config_check_fails_on_duplicate_url() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feeds = r#"feeds:
  group:
    feeds:
      - url: https://example.com/feed
        title: First
      - url: https://example.com/feed
        title: Second
"#;
    fs::write(&paths.feeds_path, feeds).expect("rewrite feeds");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("feeds")
        .arg("--config-check")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["valid"], false);
    let errors = data["errors"].as_array().expect("errors array");
    assert!(
        errors
            .iter()
            .any(|issue| issue["code"] == "DUPLICATE_FEED_URL"),
        "expected DUPLICATE_FEED_URL error"
    );
}

/// Ensures empty feed URL fails config check.
#[test]
fn config_check_fails_on_empty_url() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feeds = r#"feeds:
  group:
    feeds:
      - url: ""
        title: Empty URL
"#;
    fs::write(&paths.feeds_path, feeds).expect("rewrite feeds");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("feeds")
        .arg("--config-check")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["valid"], false);
    let errors = data["errors"].as_array().expect("errors array");
    assert!(
        errors.iter().any(|issue| issue["code"] == "EMPTY_FEED_URL"),
        "expected EMPTY_FEED_URL error"
    );
}

/// Ensures config check does not require opening the database.
#[test]
fn config_check_does_not_require_db() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(temp.path().join("missing-dir").join("db.sqlite"))
        .arg("feeds")
        .arg("--config-check")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["valid"], true);
    assert_eq!(data["checked_feeds"], 1);
}

/// Ensures feeds command reconciles and returns tags.
#[test]
fn feeds_reconcile_returns_tags() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let output = feeder_cmd_json()
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

    let data = extract_ok_data(&output);
    let feeds = data
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

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("feeds")
        .assert()
        .success();

    let output = feeder_cmd_json()
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

    let data = extract_ok_data(&output);
    let tags = data
        .get("tags")
        .and_then(|value| value.as_array())
        .expect("tags array");
    let tag_values: Vec<String> = tags
        .iter()
        .map(|tag| tag.as_str().unwrap().to_string())
        .collect();
    assert_eq!(tag_values, vec!["rust", "tech", "unread"]);
}

/// Ensures invalid TOML config is reported as CONFIG_ERROR in JSON mode.
#[test]
fn fatal_invalid_toml_syntax_is_enveloped() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config.toml");
    fs::write(&config_path, "unread_tag = ").expect("write config");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&config_path)
        .arg("tags")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "CONFIG_ERROR");
    assert_eq!(error["retry"], false);
}

/// Ensures missing feeds.yaml is reported as CONFIG_ERROR in JSON mode.
#[test]
fn fatal_missing_feeds_yaml_is_enveloped() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("missing.yaml");
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
    fs::write(&config_path, config).expect("write config");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&config_path)
        .arg("--db")
        .arg(&db_path)
        .arg("feeds")
        .arg("--config-check")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "CONFIG_ERROR");
    assert_eq!(error["retry"], false);
}

/// Ensures invalid feeds.yaml is reported as CONFIG_ERROR in JSON mode.
#[test]
fn fatal_invalid_feeds_yaml_is_enveloped() {
    let temp = TempDir::new().expect("tempdir");
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
    fs::write(&config_path, config).expect("write config");
    fs::write(&feeds_path, "feeds: [").expect("write feeds");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&config_path)
        .arg("--db")
        .arg(&db_path)
        .arg("feeds")
        .arg("--config-check")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "CONFIG_ERROR");
    assert_eq!(error["retry"], false);
}

/// Ensures feeds.yaml without top-level `feeds` key is rejected as CONFIG_ERROR.
#[test]
fn fatal_feeds_yaml_missing_top_level_feeds_is_enveloped() {
    let temp = TempDir::new().expect("tempdir");
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
    fs::write(&config_path, config).expect("write config");
    fs::write(&feeds_path, "auto_tags: []").expect("write feeds");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&config_path)
        .arg("--db")
        .arg(&db_path)
        .arg("feeds")
        .arg("--config-check")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "CONFIG_ERROR");
    assert_eq!(error["retry"], false);
}

/// Ensures unread token maps to configured unread tag.
#[test]
fn unread_token_respects_config_unread_tag() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_with_unread_tag(&temp, "fresh");

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("unread")
        .arg("--sort")
        .arg("first_seen_desc")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    let items = data["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2);
    for item in items {
        let tags = item["tags"].as_array().expect("tags array");
        let tag_values: Vec<String> = tags
            .iter()
            .map(|tag| tag.as_str().unwrap().to_string())
            .collect();
        assert!(tag_values.contains(&"fresh".to_string()));
        assert!(!tag_values.contains(&"unread".to_string()));
    }
}

/// Ensures list command renders human-readable plain output.
#[test]
fn list_plain_is_human_readable() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let output = feeder_cmd_plain()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("2")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_plain_output(&output);
    let output_str = String::from_utf8_lossy(&output);
    assert!(output_str.contains("First Entry"));
    assert!(output_str.contains("Second Entry"));
}

/// Ensures view command renders human-readable plain output.
#[test]
fn view_plain_is_human_readable() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let conn = Connection::open(&paths.db_path).expect("open db");
    let entry_id: i64 = conn
        .query_row(
            "SELECT id FROM entries WHERE title = 'First Entry'",
            [],
            |row| row.get(0),
        )
        .expect("entry id");

    let output = feeder_cmd_plain()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("view")
        .arg(entry_id.to_string())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_plain_output(&output);
    let output_str = String::from_utf8_lossy(&output);
    assert!(output_str.contains("First Entry"));
    assert!(output_str.contains("https://example.com/1"));
}

/// Ensures sync ingests entries, applies tags, and reports counts.
#[test]
fn sync_ingests_entries_and_tags() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["status"], "completed");
    assert_eq!(data["fetched"], 1);
    assert_eq!(data["failed"], 0);
    assert_eq!(data["new_entries"], 2);
    assert!(data["errors"].as_array().expect("errors array").is_empty());

    let conn = Connection::open(&paths.db_path).expect("open db");
    let entry_count: i64 = conn
        .query_row("SELECT COUNT(1) FROM entries", [], |row| row.get(0))
        .expect("entries count");
    assert_eq!(entry_count, 2);

    let unread_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE t.name = 'unread'",
            [],
            |row| row.get(0),
        )
        .expect("unread count");
    assert_eq!(unread_count, 2);

    let tech_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE t.name = 'tech'",
            [],
            |row| row.get(0),
        )
        .expect("tech count");
    assert_eq!(tech_count, 2);

    let hot_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE t.name = 'hot'",
            [],
            |row| row.get(0),
        )
        .expect("hot count");
    assert_eq!(hot_count, 1);
}

/// Ensures sync writes content to filesystem storage.
#[test]
fn sync_writes_content_to_fs_store() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_fs(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let conn = Connection::open(&paths.db_path).expect("open db");
    let (storage, reference, content): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT storage, ref, content FROM entry_contents ORDER BY entry_id LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("entry_contents");
    assert_eq!(storage, "fs");
    let reference = reference.expect("ref");
    assert!(content.is_none());

    let prefix = reference.get(0..2).expect("prefix");
    let path = Path::new(&paths.data_dir).join(prefix).join(&reference);
    assert!(path.exists());
}

/// Ensures sync reports partial failures without exiting.
#[test]
fn sync_reports_partial_failed() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_failure_fixture_files(&temp);

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["status"], "partial_failed");
    assert_eq!(data["fetched"], 2);
    assert_eq!(data["failed"], 1);
    let errors = data["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["code"], "PARSE_FAILED");
}

/// Ensures sync reports failed when all feeds fail.
#[test]
fn sync_reports_failed_when_all_feeds_fail() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_all_failed_fixture_files(&temp);

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["status"], "failed");
    assert_eq!(data["fetched"], 1);
    assert_eq!(data["failed"], 1);
    let errors = data["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1);
}

/// Ensures list returns paginated results with tag filters.
#[test]
fn list_returns_paginated_results() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let conn = Connection::open(&paths.db_path).expect("open db");
    let tech_entries: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE t.name = 'tech'",
            [],
            |row| row.get(0),
        )
        .expect("tech count");
    assert_eq!(tech_entries, 2);

    let unread_entries: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE t.name = 'unread'",
            [],
            |row| row.get(0),
        )
        .expect("unread count");
    assert_eq!(unread_entries, 2);

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("unread tag:tech")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["total_hits"], 2);
    assert_eq!(data["items"].as_array().expect("items array").len(), 1);
    let cursor = data["next_cursor"].as_str().expect("next_cursor");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("unread tag:tech")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("1")
        .arg("--cursor")
        .arg(cursor)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["total_hits"], 2);
    assert_eq!(data["items"].as_array().expect("items array").len(), 1);
    assert!(data["next_cursor"].is_null());
}

/// Ensures feed filters work by id and title.
#[test]
fn list_filters_by_feed() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let conn = Connection::open(&paths.db_path).expect("open db");
    let feed_id: i64 = conn
        .query_row(
            "SELECT id FROM feeds WHERE title = 'Example Feed'",
            [],
            |row| row.get(0),
        )
        .expect("feed id");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg(format!("feed:{feed_id}"))
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("10")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["total_hits"], 2);

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("feed:\"Example Feed\"")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("10")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["total_hits"], 2);
}

/// Ensures title filters work.
#[test]
fn list_filters_by_title() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("title:\"First\"")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("10")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["total_hits"], 1);
    let items = data["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "First Entry");
}

/// Ensures date filters are applied to effective date.
#[test]
fn list_filters_by_date_range() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("after:2024-01-02")
        .arg("--sort")
        .arg("date_desc")
        .arg("--limit")
        .arg("10")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data = extract_ok_data(&output);
    assert_eq!(data["total_hits"], 1);

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("before:2024-01-02")
        .arg("--sort")
        .arg("date_desc")
        .arg("--limit")
        .arg("10")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data = extract_ok_data(&output);
    assert_eq!(data["total_hits"], 1);
}

/// Ensures cursor mismatches are rejected.
#[test]
fn list_rejects_mismatched_cursor() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("unread")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    let cursor = data["next_cursor"].as_str().expect("cursor");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("tag:tech")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("1")
        .arg("--cursor")
        .arg(cursor)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let code = extract_error_code(&output);
    assert_eq!(code, "INVALID_QUERY");
}

/// Ensures invalid cursors are rejected with fatal error.
#[test]
fn list_rejects_invalid_cursor_format() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("list")
        .arg("--query")
        .arg("unread")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("1")
        .arg("--cursor")
        .arg("not-a-cursor")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "INVALID_QUERY");
    assert_eq!(error["retry"], false);
}

/// Ensures fatal config errors return JSON envelope and non-zero exit code.
#[test]
fn fatal_config_error_is_enveloped() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config.toml");
    let db_path = temp.path().join("db.sqlite");

    let config = format!(
        r#"unread_tag = "unread"

[database]
path = "{}"

[feeds]
source = "{}"

[cli]
output = "bogus"
"#,
        db_path.display(),
        temp.path().join("feeds.yaml").display()
    );

    fs::write(&config_path, config).expect("write config");
    fs::write(temp.path().join("feeds.yaml"), "feeds: {}").expect("write feeds");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&config_path)
        .arg("--db")
        .arg(&db_path)
        .arg("tags")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "CONFIG_ERROR");
    assert_eq!(error["retry"], false);
}

/// Ensures locked database errors are fatal and retryable.
#[test]
fn db_locked_returns_retry_true() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let conn = Connection::open(&paths.db_path).expect("open db");
    conn.execute("BEGIN EXCLUSIVE", []).expect("lock db");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("tags")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "DB_LOCKED");
    assert_eq!(error["retry"], true);
}

/// Ensures view returns entry details with tags.
#[test]
fn view_returns_entry_detail() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let conn = Connection::open(&paths.db_path).expect("open db");
    let entry_id: i64 = conn
        .query_row(
            "SELECT id FROM entries WHERE title = 'First Entry'",
            [],
            |row| row.get(0),
        )
        .expect("entry id");

    let output = feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("view")
        .arg(entry_id.to_string())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["id"], entry_id);
    assert_eq!(data["feed_title"], "Example Feed");
    let tags = data["tags"].as_array().expect("tags array");
    let tag_values: Vec<String> = tags
        .iter()
        .map(|tag| tag.as_str().unwrap().to_string())
        .collect();
    assert!(tag_values.contains(&"unread".to_string()));
    assert!(tag_values.contains(&"tech".to_string()));
}

/// Ensures mark commands update tags.
#[test]
fn mark_updates_tags() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let conn = Connection::open(&paths.db_path).expect("open db");
    let entry_ids: Vec<i64> = conn
        .prepare("SELECT id FROM entries ORDER BY id")
        .expect("prepare")
        .query_map([], |row| row.get(0))
        .expect("rows")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(entry_ids.len(), 2);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("mark")
        .arg("read")
        .arg(entry_ids[0].to_string())
        .arg(entry_ids[1].to_string())
        .assert()
        .success();

    let unread_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE t.name = 'unread'",
            [],
            |row| row.get(0),
        )
        .expect("unread count");
    assert_eq!(unread_count, 0);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("mark")
        .arg("unread")
        .arg(entry_ids[0].to_string())
        .assert()
        .success();

    let unread_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE t.name = 'unread'",
            [],
            |row| row.get(0),
        )
        .expect("unread count");
    assert_eq!(unread_count, 1);

    feeder_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--db")
        .arg(&paths.db_path)
        .arg("mark")
        .arg("tag")
        .arg(entry_ids[0].to_string())
        .arg("--add")
        .arg("foo,bar")
        .arg("--remove")
        .arg("tech")
        .assert()
        .success();

    let foo_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE t.name = 'foo'",
            [],
            |row| row.get(0),
        )
        .expect("foo count");
    assert_eq!(foo_count, 1);

    let tech_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE t.name = 'tech'",
            [],
            |row| row.get(0),
        )
        .expect("tech count");
    assert_eq!(tech_count, 1);
}

/// Fixture file paths for CLI tests.
struct FixturePaths {
    config_path: String,
    db_path: String,
    feeds_path: String,
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
        feeds_path: feeds_path.display().to_string(),
    }
}

/// Fixture file paths for sync tests.
struct SyncFixturePaths {
    config_path: String,
    db_path: String,
}

/// Writes config, feeds, and feed XML for sync tests.
fn write_sync_fixture_files(temp: &TempDir) -> SyncFixturePaths {
    write_sync_fixture_files_with_unread_tag(temp, "unread")
}

/// Writes config, feeds, and feed XML for sync tests with custom unread tag.
fn write_sync_fixture_files_with_unread_tag(temp: &TempDir, unread_tag: &str) -> SyncFixturePaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    let feed_path = temp.path().join("feed.xml");

    let feed_xml = r#"<?xml version="1.0"?>
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

    let config = format!(
        r#"unread_tag = "{unread_tag}"

[database]
path = "{}"

[feeds]
source = "{}"

[sync]
parallel = 1
timeout = 5
user_agent = "feeder-test/0.1.0"
retry_count = 0
retry_delay = 0

[storage]
content_store = "db"
"#,
        db_path.display(),
        feeds_path.display()
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
    fs::write(&feed_path, feed_xml).expect("write feed");

    SyncFixturePaths {
        config_path: config_path.display().to_string(),
        db_path: db_path.display().to_string(),
    }
}

/// Fixture file paths for fs-content sync tests.
struct SyncFixtureFsPaths {
    config_path: String,
    db_path: String,
    data_dir: String,
}

/// Writes config, feeds, and feed XML for fs-content sync tests.
fn write_sync_fixture_files_fs(temp: &TempDir) -> SyncFixtureFsPaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    let feed_path = temp.path().join("feed.xml");
    let data_dir = temp.path().join("data");

    let feed_xml = r#"<?xml version="1.0"?>
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

    let config = format!(
        r#"unread_tag = "unread"

[database]
path = "{}"

[feeds]
source = "{}"

[sync]
parallel = 1
timeout = 5
user_agent = "feeder-test/0.1.0"
retry_count = 0
retry_delay = 0

[storage]
content_store = "fs"
data_dir = "{}"
"#,
        db_path.display(),
        feeds_path.display(),
        data_dir.display()
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
    fs::write(&feed_path, feed_xml).expect("write feed");

    SyncFixtureFsPaths {
        config_path: config_path.display().to_string(),
        db_path: db_path.display().to_string(),
        data_dir: data_dir.display().to_string(),
    }
}

/// Writes config, feeds, and invalid feed XML for partial failure tests.
fn write_sync_failure_fixture_files(temp: &TempDir) -> SyncFixturePaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    let feed_ok_path = temp.path().join("feed_ok.xml");
    let feed_bad_path = temp.path().join("feed_bad.xml");

    let feed_ok_xml = r#"<?xml version="1.0"?>
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

    let config = format!(
        r#"unread_tag = "unread"

[database]
path = "{}"

[feeds]
source = "{}"

[sync]
parallel = 1
timeout = 5
user_agent = "feeder-test/0.1.0"
retry_count = 0
retry_delay = 0

[storage]
content_store = "db"
"#,
        db_path.display(),
        feeds_path.display()
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
    fs::write(&feed_ok_path, feed_ok_xml).expect("write ok feed");
    fs::write(&feed_bad_path, "not xml").expect("write bad feed");

    SyncFixturePaths {
        config_path: config_path.display().to_string(),
        db_path: db_path.display().to_string(),
    }
}

/// Writes config and invalid feeds for all-failure tests.
fn write_sync_all_failed_fixture_files(temp: &TempDir) -> SyncFixturePaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    let feed_bad_path = temp.path().join("feed_bad.xml");

    let config = format!(
        r#"unread_tag = "unread"

[database]
path = "{}"

[feeds]
source = "{}"

[sync]
parallel = 1
timeout = 5
user_agent = "feeder-test/0.1.0"
retry_count = 0
retry_delay = 0

[storage]
content_store = "db"
"#,
        db_path.display(),
        feeds_path.display()
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
