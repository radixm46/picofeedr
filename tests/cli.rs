//! CLI integration tests.

mod support;

use assert_cmd::cargo::cargo_bin_cmd;
use rusqlite::Connection;
use std::fs;
use std::path::Path;
use support::assertions::{
    assert_error_envelope, assert_plain_contract, extract_error_code, extract_error_payload,
    extract_ok_data,
};
use support::fixtures::{
    acquire_exclusive_db_lock, write_fixture_files, write_sync_all_failed_fixture_files,
    write_sync_failure_fixture_files, write_sync_fixture_files, write_sync_fixture_files_fs,
    write_sync_fixture_files_with_query_limits, write_sync_fixture_files_with_unread_tag,
};
use tempfile::TempDir;

/// Creates a picofeedr command configured for JSON output.
fn picofeedr_cmd_json() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("picofeedr");
    cmd.arg("--output").arg("json");
    cmd
}

/// Creates a picofeedr command configured for plain output.
fn picofeedr_cmd_plain() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("picofeedr");
    cmd.arg("--output").arg("plain");
    cmd
}

/// Ensures feeds --config-check returns validation report fields.
#[test]
fn config_check_returns_validation_report() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
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

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("feeds")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

/// Case definition for fatal configuration envelope validation.
struct FatalEnvelopeCase {
    /// Human-readable case name for diagnostics.
    name: &'static str,
    /// Case setup function.
    setup: fn(&TempDir) -> FatalEnvelopeInputs,
}

/// Command inputs generated for fatal envelope test cases.
struct FatalEnvelopeInputs {
    /// Config path passed to the command.
    config_path: String,
    /// Optional database path passed to the command.
    db_path: Option<String>,
    /// Command and arguments after global options.
    command_args: Vec<&'static str>,
}

/// Ensures fatal configuration errors consistently keep the JSON error envelope contract.
#[test]
fn fatal_config_cases_are_enveloped() {
    let cases = vec![
        FatalEnvelopeCase {
            name: "invalid toml syntax",
            setup: fatal_case_invalid_toml_syntax,
        },
        FatalEnvelopeCase {
            name: "missing feeds yaml",
            setup: fatal_case_missing_feeds_yaml,
        },
        FatalEnvelopeCase {
            name: "invalid feeds yaml",
            setup: fatal_case_invalid_feeds_yaml,
        },
        FatalEnvelopeCase {
            name: "feeds yaml without feeds key",
            setup: fatal_case_missing_top_level_feeds_key,
        },
        FatalEnvelopeCase {
            name: "invalid cli output value in config",
            setup: fatal_case_invalid_cli_output_value,
        },
    ];

    for case in cases {
        let temp = TempDir::new().expect("tempdir");
        let inputs = (case.setup)(&temp);
        let mut cmd = picofeedr_cmd_json();
        cmd.arg("--config").arg(&inputs.config_path);
        if let Some(db_path) = inputs.db_path {
            cmd.arg("--storage-root").arg(db_root(&db_path));
        }
        let output = cmd
            .args(&inputs.command_args)
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        assert_error_envelope(&output, "CONFIG_ERROR", false);
        assert_eq!(
            extract_error_code(&output),
            "CONFIG_ERROR",
            "case={}",
            case.name
        );
    }
}

/// Creates inputs for the invalid TOML fatal-envelope case.
fn fatal_case_invalid_toml_syntax(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    fs::write(&config_path, "unread_tag = ").expect("write config");
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: None,
        command_args: vec!["tags"],
    }
}

/// Creates inputs for the missing feeds YAML fatal-envelope case.
fn fatal_case_missing_feeds_yaml(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("missing.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: Some(db_path.display().to_string()),
        command_args: vec!["feeds", "--config-check"],
    }
}

/// Creates inputs for the invalid feeds YAML fatal-envelope case.
fn fatal_case_invalid_feeds_yaml(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);
    fs::write(&feeds_path, "feeds: [").expect("write feeds");
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: Some(db_path.display().to_string()),
        command_args: vec!["feeds", "--config-check"],
    }
}

/// Creates inputs for the missing `feeds` top-level key fatal-envelope case.
fn fatal_case_missing_top_level_feeds_key(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);
    fs::write(&feeds_path, "auto_tags: []").expect("write feeds");
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: Some(db_path.display().to_string()),
        command_args: vec!["feeds", "--config-check"],
    }
}

/// Creates inputs for the invalid CLI output config fatal-envelope case.
fn fatal_case_invalid_cli_output_value(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    let db_path = temp.path().join("db.sqlite");
    let feeds_path = temp.path().join("feeds.yaml");
    let config = format!(
        r#"unread_tag = "unread"

[feeds]
source = "{}"

[storage]
root_dir = "{}"

[cli]
output = "bogus"
"#,
        feeds_path.display(),
        temp.path().display()
    );
    fs::write(&config_path, config).expect("write config");
    fs::write(feeds_path, "feeds: {}").expect("write feeds");
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: Some(db_path.display().to_string()),
        command_args: vec!["tags"],
    }
}

/// Writes a minimal config file pointing to a specific feeds source.
fn write_config_with_feeds_source(config_path: &Path, db_path: &Path, feeds_path: &Path) {
    let root_dir = db_path
        .parent()
        .expect("db path should include a parent directory");
    let config = format!(
        r#"unread_tag = "unread"

[feeds]
source = "{}"

[storage]
root_dir = "{}"
"#,
        feeds_path.display(),
        root_dir.display()
    );
    fs::write(config_path, config).expect("write config");
}

/// Ensures unread token maps to configured unread tag.
#[test]
fn unread_token_respects_config_unread_tag() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_with_unread_tag(&temp, "fresh");

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_plain()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    assert_plain_contract(&output, &["First Entry", "Second Entry"]);
}

/// Ensures view command renders human-readable plain output.
#[test]
fn view_plain_is_human_readable() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let entry_id = entry_id_by_title(&paths.config_path, &paths.db_path, "First");

    let output = picofeedr_cmd_plain()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("view")
        .arg(entry_id.to_string())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_plain_contract(&output, &["First Entry", "https://example.com/1", "feed"]);
}

/// Ensures sync reports contract fields and query-visible outcomes.
#[test]
fn sync_ingests_entries_and_tags() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    let unread_data = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread_data["total_hits"], 2);

    let tech_data = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    assert_eq!(tech_data["total_hits"], 2);

    let hot_data = list_query_json(&paths.config_path, &paths.db_path, "tag:hot");
    assert_eq!(hot_data["total_hits"], 1);
}

/// Ensures sync writes content to filesystem storage.
#[test]
fn sync_writes_content_to_fs_store() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_fs(&temp);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

/// Ensures --storage-root override keeps db.sqlite and data/ under the same root for fs storage.
#[test]
fn storage_root_override_updates_fs_storage_root() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_fs(&temp);
    let override_root = temp.path().join("override-root");
    fs::create_dir_all(&override_root).expect("create override root");
    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(&override_root)
        .arg("sync")
        .assert()
        .success();

    let override_db_path = override_root.join("db.sqlite");
    let conn = Connection::open(&override_db_path).expect("open overridden db");
    let (storage, reference): (String, Option<String>) = conn
        .query_row(
            "SELECT storage, ref FROM entry_contents ORDER BY entry_id LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("entry_contents");
    assert_eq!(storage, "fs");
    let reference = reference.expect("ref");

    let prefix = reference.get(0..2).expect("prefix");
    let overridden_path = override_root.join("data").join(prefix).join(&reference);
    assert!(overridden_path.exists());

    let original_path = Path::new(&paths.data_dir).join(prefix).join(&reference);
    assert!(!original_path.exists());
}

/// Ensures sync reports partial failures without exiting.
#[test]
fn sync_reports_partial_failed() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_failure_fixture_files(&temp);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

/// Ensures tag expression operators and precedence are applied.
#[test]
fn list_filters_by_tag_expression() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("tag:(hot|tech)&!hot")
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
    assert_eq!(items[0]["title"], "Second Entry");

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("tag:hot|tech&!hot")
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

/// Ensures -tag expression alias applies top-level NOT.
#[test]
fn list_accepts_minus_tag_expression_alias() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("tag:tech -tag:hot|rust")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data = extract_ok_data(&output);
    assert_eq!(data["total_hits"], 1);
    let items = data["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "Second Entry");
}

/// Ensures feed filters work by id and title.
#[test]
fn list_filters_by_feed() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let feed_id = first_feed_id_from_list(&paths.config_path, &paths.db_path);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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

/// Ensures list uses config query.default_limit when --limit is omitted.
#[test]
fn list_uses_config_default_limit_when_limit_omitted() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_with_query_limits(&temp, "unread", 1, 5);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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
    assert_eq!(items.len(), 1);
    assert!(data["next_cursor"].is_string());
}

/// Ensures list rejects limits over config query.max_limit.
#[test]
fn list_rejects_limit_over_max_limit() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_with_query_limits(&temp, "unread", 1, 5);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("unread")
        .arg("--limit")
        .arg("6")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let code = extract_error_code(&output);
    assert_eq!(code, "INVALID_QUERY");
}

/// Ensures list rejects zero limit.
#[test]
fn list_rejects_zero_limit() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_with_query_limits(&temp, "unread", 1, 5);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("unread")
        .arg("--limit")
        .arg("0")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let code = extract_error_code(&output);
    assert_eq!(code, "INVALID_QUERY");
}

/// Ensures config rejects query.default_limit = 0.
#[test]
fn fatal_config_rejects_zero_default_limit() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_with_query_limits(&temp, "unread", 0, 5);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("unread")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let code = extract_error_code(&output);
    assert_eq!(code, "CONFIG_ERROR");
}

/// Ensures config rejects query.max_limit = 0.
#[test]
fn fatal_config_rejects_zero_max_limit() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_with_query_limits(&temp, "unread", 1, 0);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("unread")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let code = extract_error_code(&output);
    assert_eq!(code, "CONFIG_ERROR");
}

/// Ensures config rejects query.default_limit > query.max_limit.
#[test]
fn fatal_config_rejects_default_limit_over_max_limit() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_with_query_limits(&temp, "unread", 6, 5);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("unread")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let code = extract_error_code(&output);
    assert_eq!(code, "CONFIG_ERROR");
}

/// Ensures CLI parse errors keep JSON envelope when using --output=json form.
#[test]
fn parse_error_with_output_equals_json_is_enveloped() {
    let output = cargo_bin_cmd!("picofeedr")
        .arg("--output=json")
        .arg("unknown")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_error_envelope(&output, "CONFIG_ERROR", false);
}

/// Ensures locked database errors are fatal and retryable.
#[test]
fn db_locked_returns_retry_true() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let _lock = acquire_exclusive_db_lock(&paths.db_path);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("tags")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_error_envelope(&output, "DB_LOCKED", true);
}

/// Ensures view returns entry details with tags.
#[test]
fn view_returns_entry_detail() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let entry_id = entry_id_by_title(&paths.config_path, &paths.db_path, "First");

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
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
    assert_eq!(data["title"], "First Entry");
    assert_eq!(data["link"], "https://example.com/1");
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

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let unread_data = list_query_json(&paths.config_path, &paths.db_path, "unread");
    let entry_ids = collect_item_ids(&unread_data);
    assert_eq!(entry_ids.len(), 2);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("read")
        .arg(entry_ids[0].to_string())
        .arg(entry_ids[1].to_string())
        .assert()
        .success();

    let unread_after_read = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread_after_read["total_hits"], 0);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("unread")
        .arg(entry_ids[0].to_string())
        .assert()
        .success();

    let unread_after_unread = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread_after_unread["total_hits"], 1);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("tag")
        .arg(entry_ids[0].to_string())
        .arg("--add")
        .arg("foo,bar")
        .arg("--remove")
        .arg("tech")
        .assert()
        .success();

    let foo_data = list_query_json(&paths.config_path, &paths.db_path, "tag:foo");
    assert_eq!(foo_data["total_hits"], 1);

    let tech_data = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    assert_eq!(tech_data["total_hits"], 1);
}

/// Runs `list` in JSON mode and returns its `data` object.
fn list_query_json(config_path: &str, db_path: &str, query: &str) -> serde_json::Value {
    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path)
        .arg("--storage-root")
        .arg(db_root(db_path))
        .arg("list")
        .arg("--query")
        .arg(query)
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("10")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    extract_ok_data(&output)
}

/// Resolves an entry id from a title query.
fn entry_id_by_title(config_path: &str, db_path: &str, title: &str) -> i64 {
    let data = list_query_json(config_path, db_path, &format!("title:\"{title}\""));
    let items = data["items"].as_array().expect("items array");
    items
        .first()
        .and_then(|item| item["id"].as_i64())
        .expect("entry id by title")
}

/// Resolves the first feed id from list output.
fn first_feed_id_from_list(config_path: &str, db_path: &str) -> i64 {
    let data = list_query_json(config_path, db_path, "unread");
    let items = data["items"].as_array().expect("items array");
    items
        .first()
        .and_then(|item| item["feed_id"].as_i64())
        .expect("feed id")
}

/// Resolves root_dir from a db path.
fn db_root(db_path: &str) -> String {
    Path::new(db_path)
        .parent()
        .expect("db path should include a parent directory")
        .display()
        .to_string()
}

/// Collects entry ids from list response items.
fn collect_item_ids(data: &serde_json::Value) -> Vec<i64> {
    data["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| item["id"].as_i64().expect("item id"))
        .collect()
}
