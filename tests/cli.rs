//! CLI integration tests.

mod support;

use assert_cmd::cargo::cargo_bin_cmd;
use rusqlite::Connection;
use std::fs;
use std::path::Path;
use std::process::{Command as ProcessCommand, Output, Stdio};
use support::assertions::{
    assert_envelope_status, assert_error_envelope, assert_plain_contract, extract_error_code,
    extract_error_details, extract_error_payload, extract_ok_data,
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
    let feeds = r#"picofeedr:
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
    assert_envelope_status(&output, "warning");
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
    let feeds = r#"picofeedr:
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
    assert_envelope_status(&output, "warning");
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

/// Ensures unknown top-level keys are ignored.
#[test]
fn config_check_ignores_unknown_top_level_keys() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feeds = r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
ended:
  - https://example.com/feed
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
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["valid"], true);
    assert!(data["errors"].as_array().expect("errors").is_empty());
    assert!(data["warnings"].as_array().expect("warnings").is_empty());
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
    assert!(feeds[0]["feed_id"].is_string());
    assert!(feeds[0].get("feed_key").is_none());
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

/// Ensures status returns default metadata values before write commands.
#[test]
fn status_returns_default_metadata() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let status = status_json(&paths.config_path, &paths.db_path);
    assert_eq!(status["revision"], 0);
    assert!(status["last_write_at"].is_null());
    assert_eq!(status["db_schema_version"], 1);
    assert!(status["api_version"].as_str().is_some());
    assert!(status["last_sync_at"].is_null());
    assert!(status["last_sync_status"].is_null());
}

/// Ensures feeds reconciliation increments revision metadata.
#[test]
fn status_tracks_feeds_write_revision() {
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

    let status = status_json(&paths.config_path, &paths.db_path);
    assert_eq!(status["revision"], 1);
    assert!(status["last_write_at"].as_i64().is_some());
}

/// Ensures status tracks sync and mark writes and ignores read-only commands.
#[test]
fn status_tracks_revision_and_sync_metadata() {
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

    let after_sync = status_json(&paths.config_path, &paths.db_path);
    assert_eq!(after_sync["revision"], 1);
    assert!(after_sync["last_write_at"].as_i64().is_some());
    assert!(after_sync["last_sync_at"].as_i64().is_some());
    assert_eq!(after_sync["last_sync_status"], "completed");

    let _ = list_query_json(&paths.config_path, &paths.db_path, "unread");
    let entry_id = entry_id_by_title(&paths.config_path, &paths.db_path, "First");
    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("view")
        .arg(entry_id.clone())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let _ = extract_ok_data(&output);

    let after_reads = status_json(&paths.config_path, &paths.db_path);
    assert_eq!(after_reads["revision"], after_sync["revision"]);
    assert_eq!(after_reads["last_write_at"], after_sync["last_write_at"]);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("read")
        .arg(entry_id.clone())
        .assert()
        .success();

    let after_mark = status_json(&paths.config_path, &paths.db_path);
    assert_eq!(after_mark["revision"], 2);
    assert_eq!(after_mark["last_sync_status"], "completed");
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
            name: "feeds yaml without picofeedr key",
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

/// Ensures CONFIG_ERROR exposes structured details for missing feeds file.
#[test]
fn config_error_includes_details_for_missing_feeds_yaml() {
    let temp = TempDir::new().expect("tempdir");
    let inputs = fatal_case_missing_feeds_yaml(&temp);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&inputs.config_path)
        .arg("--storage-root")
        .arg(db_root(inputs.db_path.as_deref().expect("db path")))
        .args(&inputs.command_args)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let details = extract_error_details(&output);
    assert_eq!(details["hint"], "failed_to_read_feeds_yaml");
    assert!(details["path"].as_str().is_some());
}

/// Creates inputs for the invalid feeds YAML fatal-envelope case.
fn fatal_case_invalid_feeds_yaml(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);
    fs::write(&feeds_path, "picofeedr: [").expect("write feeds");
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: Some(db_path.display().to_string()),
        command_args: vec!["feeds", "--config-check"],
    }
}

/// Creates inputs for the missing `picofeedr` top-level key fatal-envelope case.
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
    fs::write(feeds_path, "picofeedr: {}").expect("write feeds");
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
        .arg(entry_id.clone())
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
    assert_eq!(data["fetched_feed_count"], 1);
    assert_eq!(data["failed_feed_count"], 0);
    assert_eq!(data["new_entry_count"], 2);
    assert!(data["errors"].as_array().expect("errors array").is_empty());

    let unread_data = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread_data["total_count"], 2);

    let tech_data = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    assert_eq!(tech_data["total_count"], 2);

    let hot_data = list_query_json(&paths.config_path, &paths.db_path, "tag:hot");
    assert_eq!(hot_data["total_count"], 1);
}

/// Ensures sync plain output streams feed progress and final summary for successful sync.
#[test]
fn sync_plain_shows_feed_level_progress_and_final_summary() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);

    let output = picofeedr_cmd_plain()
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

    let output_str = String::from_utf8_lossy(&output);
    assert!(output_str.contains("sync:start total_feeds=1"));
    assert!(output_str.contains("sync:feed start index=1/1 url=file://"));
    assert!(output_str.contains("sync:feed ok index=1/1 url=file://"));
    assert!(output_str.contains("entries=2"));
    assert!(output_str.contains("status: completed"));
    assert!(output_str.contains("fetched_feed_count: 1 failed_feed_count: 0 new_entry_count: 2"));
}

/// Ensures legacy top-level auto_tags are ignored without warnings.
#[test]
fn sync_ignores_legacy_top_level_auto_tags() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    let feed_path = temp.path().join("feed.xml");
    let feed_url = format!("file://{}", feed_path.display());
    let feeds = format!(
        r#"picofeedr:
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
    fs::write(temp.path().join("feeds.yaml"), feeds).expect("rewrite feeds");

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    let hot_data = list_query_json(&paths.config_path, &paths.db_path, "tag:hot");
    assert_eq!(hot_data["total_count"], 0);
}

/// Ensures subgroup auto_tags apply only to descendant feeds.
#[test]
fn subgroup_auto_tags_apply_only_to_descendants() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);

    let feed_a = temp.path().join("a.xml");
    let feed_b = temp.path().join("b.xml");
    fs::write(&feed_a, sample_feed_xml("steam-a", "Steam Weekly")).expect("write feed a");
    fs::write(&feed_b, sample_feed_xml("steam-b", "Steam Weekly")).expect("write feed b");

    let feeds = format!(
        r#"picofeedr:
  tech:
    auto_tags:
      - title_contains: [Steam]
        add_tags: [sale]
    feeds:
      - url: file://{}
  news:
    feeds:
      - url: file://{}
"#,
        feed_a.display(),
        feed_b.display()
    );
    fs::write(&feeds_path, feeds).expect("write feeds");

    picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path.display().to_string())
        .arg("--storage-root")
        .arg(db_root(db_path.to_str().expect("db path")))
        .arg("sync")
        .assert()
        .success();

    let sale_data = list_query_json(
        config_path.to_str().expect("config path"),
        db_path.to_str().expect("db path"),
        "tag:sale",
    );
    assert_eq!(sale_data["total_count"], 1);
}

/// Ensures parent and child auto_tags are both applied.
#[test]
fn parent_and_child_auto_tags_are_both_applied() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);

    let feed = temp.path().join("combo.xml");
    fs::write(&feed, sample_feed_xml("combo", "Steam Digest")).expect("write feed");
    let feeds = format!(
        r#"picofeedr:
  parent:
    auto_tags:
      - title_contains: [Digest]
        add_tags: [parent]
    child:
      auto_tags:
        - title_contains: [Steam]
          add_tags: [child]
      feeds:
        - url: file://{}
"#,
        feed.display()
    );
    fs::write(&feeds_path, feeds).expect("write feeds");

    picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path.display().to_string())
        .arg("--storage-root")
        .arg(db_root(db_path.to_str().expect("db path")))
        .arg("sync")
        .assert()
        .success();

    let parent_data = list_query_json(
        config_path.to_str().expect("config path"),
        db_path.to_str().expect("db path"),
        "tag:parent",
    );
    assert_eq!(parent_data["total_count"], 1);
    let child_data = list_query_json(
        config_path.to_str().expect("config path"),
        db_path.to_str().expect("db path"),
        "tag:child",
    );
    assert_eq!(child_data["total_count"], 1);
}

/// Ensures sibling groups are not affected by subgroup auto_tags.
#[test]
fn sibling_group_not_affected_by_subgroup_auto_tags() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);

    let tech_feed = temp.path().join("tech.xml");
    let sibling_feed = temp.path().join("sibling.xml");
    fs::write(&tech_feed, sample_feed_xml("tech", "Steam Weekly")).expect("write tech feed");
    fs::write(&sibling_feed, sample_feed_xml("sibling", "Steam Weekly"))
        .expect("write sibling feed");

    let feeds = format!(
        r#"picofeedr:
  parent:
    tech:
      auto_tags:
        - title_contains: [Steam]
          add_tags: [sale]
      feeds:
        - url: file://{}
    sibling:
      feeds:
        - url: file://{}
"#,
        tech_feed.display(),
        sibling_feed.display()
    );
    fs::write(&feeds_path, feeds).expect("write feeds");

    picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path.display().to_string())
        .arg("--storage-root")
        .arg(db_root(db_path.to_str().expect("db path")))
        .arg("sync")
        .assert()
        .success();

    let sale_data = list_query_json(
        config_path.to_str().expect("config path"),
        db_path.to_str().expect("db path"),
        "tag:sale",
    );
    assert_eq!(sale_data["total_count"], 1);
}

/// Ensures duplicate tags from multiple matching rules are deduplicated.
#[test]
fn duplicate_tags_from_multiple_matching_rules_are_deduped() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);

    let feed = temp.path().join("dup.xml");
    fs::write(&feed, sample_feed_xml("dup", "Steam Digest")).expect("write feed");
    let feeds = format!(
        r#"picofeedr:
  root:
    auto_tags:
      - title_contains: [Steam]
        add_tags: [dup]
    child:
      auto_tags:
        - title_contains: [Digest]
          add_tags: [dup]
      feeds:
        - url: file://{}
"#,
        feed.display()
    );
    fs::write(&feeds_path, feeds).expect("write feeds");

    picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path.display().to_string())
        .arg("--storage-root")
        .arg(db_root(db_path.to_str().expect("db path")))
        .arg("sync")
        .assert()
        .success();

    let dup_data = list_query_json(
        config_path.to_str().expect("config path"),
        db_path.to_str().expect("db path"),
        "tag:dup",
    );
    assert_eq!(dup_data["total_count"], 1);
    let item = dup_data["items"]
        .as_array()
        .expect("items")
        .first()
        .expect("first item");
    let dup_count = item["tags"]
        .as_array()
        .expect("tags")
        .iter()
        .filter(|tag| tag.as_str() == Some("dup"))
        .count();
    assert_eq!(dup_count, 1);
}

/// Ensures config check reports nested auto_tag rule path.
#[test]
fn config_check_reports_invalid_nested_auto_tag_rule_path() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feeds = r#"picofeedr:
  group:
    auto_tags:
      - title_contains: [Steam]
        add_tags: []
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
    let errors = data["errors"].as_array().expect("errors");
    assert!(
        errors
            .iter()
            .any(|issue| issue["path"] == "picofeedr.group.auto_tags[0].add_tags"),
        "expected nested auto_tags path"
    );
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
            "SELECT storage, ref, content FROM entry_contents ORDER BY entry_pk LIMIT 1",
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
            "SELECT storage, ref FROM entry_contents ORDER BY entry_pk LIMIT 1",
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
    assert_envelope_status(&output, "warning");
    assert_eq!(data["status"], "partial_failed");
    assert_eq!(data["fetched_feed_count"], 2);
    assert_eq!(data["failed_feed_count"], 1);
    let errors = data["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["code"], "PARSE_FAILED");
}

/// Ensures sync plain output reports progress and feed-level error details for partial failures.
#[test]
fn sync_plain_reports_partial_failed_with_feed_error_lines() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_failure_fixture_files(&temp);

    let output = picofeedr_cmd_plain()
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

    let output_str = String::from_utf8_lossy(&output);
    assert!(output_str.contains("sync:start total_feeds=2"));
    assert!(output_str.contains("sync:feed start index=1/2 url=file://"));
    assert!(output_str.contains("sync:feed start index=2/2 url=file://"));
    assert!(output_str.contains("sync:feed ok index=1/2 url=file://"));
    assert!(output_str.contains("sync:feed error index=2/2 url=file://"));
    assert!(output_str.contains("code=PARSE_FAILED retryable=false"));
    assert!(output_str.contains("status: partial_failed"));
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
    assert_envelope_status(&output, "warning");
    assert_eq!(data["status"], "failed");
    assert_eq!(data["fetched_feed_count"], 1);
    assert_eq!(data["failed_feed_count"], 1);
    let errors = data["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1);
}

/// Ensures sync plain output reports feed-level error details when all feeds fail.
#[test]
fn sync_plain_reports_failed_with_feed_error_lines() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_all_failed_fixture_files(&temp);

    let output = picofeedr_cmd_plain()
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

    let output_str = String::from_utf8_lossy(&output);
    assert!(output_str.contains("sync:start total_feeds=1"));
    assert!(output_str.contains("sync:feed start index=1/1 url=file://"));
    assert!(output_str.contains("sync:feed error index=1/1 url=file://"));
    assert!(output_str.contains("code=PARSE_FAILED retryable=false"));
    assert!(output_str.contains("status: failed"));
    assert!(output_str.contains("errors: 1"));
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
    assert_eq!(data["total_count"], 2);
    assert_eq!(data["items"].as_array().expect("items array").len(), 1);
    assert!(data["revision"].as_i64().is_some());
    assert!(data["last_write_at"].as_i64().is_some());
    let cursor = data["next_page_token"].as_str().expect("next_cursor");

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
    assert_eq!(data["total_count"], 2);
    assert_eq!(data["items"].as_array().expect("items array").len(), 1);
    assert!(data["revision"].as_i64().is_some());
    assert!(data["last_write_at"].as_i64().is_some());
    assert!(data["next_page_token"].is_null());
}

/// Ensures list snapshot metadata is consistent with status metadata.
#[test]
fn list_snapshot_matches_status_metadata() {
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

    let status = status_json(&paths.config_path, &paths.db_path);
    let data = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(data["revision"], status["revision"]);
    assert_eq!(data["last_write_at"], status["last_write_at"]);
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
    assert_eq!(data["total_count"], 1);
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
    assert_eq!(data["total_count"], 2);
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
    assert_eq!(data["total_count"], 1);
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
    assert_eq!(data["total_count"], 2);
    let feeds = data["feeds"].as_array().expect("feeds array");
    assert_eq!(feeds.len(), 1);
    assert_eq!(feeds[0]["feed_id"], feed_id);
    assert_eq!(feeds[0]["title"], "Example Feed");

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
    assert_eq!(data["total_count"], 2);
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
    assert_eq!(data["total_count"], 1);
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
    assert_eq!(data["total_count"], 1);

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
    assert_eq!(data["total_count"], 1);
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
    let cursor = data["next_page_token"].as_str().expect("cursor");

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

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "INVALID_QUERY");
    let details = extract_error_details(&output);
    assert_eq!(details["kind"], "invalid_cursor");
    assert_eq!(details["field"], "cursor");
    assert_eq!(details["hint"], "cursor_mismatch");
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
    assert_eq!(error["retryable"], false);
    let details = extract_error_details(&output);
    assert_eq!(details["kind"], "invalid_cursor");
    assert_eq!(details["field"], "cursor");
    assert_eq!(details["hint"], "cursor_json_decode_failed");
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
    assert!(data["next_page_token"].is_string());
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

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "INVALID_QUERY");
    let details = extract_error_details(&output);
    assert_eq!(details["kind"], "limit_out_of_range");
    assert_eq!(details["field"], "limit");
    assert_eq!(details["hint"], "limit_exceeds_configured_max_limit");
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

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "INVALID_QUERY");
    let details = extract_error_details(&output);
    assert_eq!(details["kind"], "limit_out_of_range");
    assert_eq!(details["field"], "limit");
    assert_eq!(details["hint"], "limit_must_be_greater_than_zero");
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

/// Ensures plain output exits successfully when stdout is closed by downstream.
#[test]
fn plain_output_succeeds_when_stdout_is_closed() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let storage_root = db_root(&paths.db_path);

    let output = run_with_closed_stdout(vec![
        "--output".to_string(),
        "plain".to_string(),
        "--config".to_string(),
        paths.config_path.clone(),
        "--storage-root".to_string(),
        storage_root,
        "feeds".to_string(),
    ]);

    assert!(
        output.status.success(),
        "expected success, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Ensures json output exits successfully when stdout is closed by downstream.
#[test]
fn json_output_succeeds_when_stdout_is_closed() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let storage_root = db_root(&paths.db_path);

    let output = run_with_closed_stdout(vec![
        "--output".to_string(),
        "json".to_string(),
        "--config".to_string(),
        paths.config_path.clone(),
        "--storage-root".to_string(),
        storage_root,
        "feeds".to_string(),
    ]);

    assert!(
        output.status.success(),
        "expected success, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Ensures debug mode prints broken-pipe diagnostics to stderr.
#[test]
fn broken_pipe_emits_debug_diagnostic() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let storage_root = db_root(&paths.db_path);

    let output = run_with_closed_stdout(vec![
        "--output".to_string(),
        "plain".to_string(),
        "--debug".to_string(),
        "--config".to_string(),
        paths.config_path.clone(),
        "--storage-root".to_string(),
        storage_root,
        "feeds".to_string(),
    ]);

    assert!(
        output.status.success(),
        "expected success, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("broken pipe"),
        "expected broken pipe diagnostics, got stderr={stderr}"
    );
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
    let details = extract_error_details(&output);
    assert_eq!(details["retry_after_ms"], 200);
    assert!(
        details["sqlite_code"].is_string() || details["sqlite_code"].is_null(),
        "sqlite_code must be string|null"
    );
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
        .arg(entry_id.clone())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["entry_id"], entry_id);
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

/// Ensures missing entry view returns ENTRY_NOT_FOUND with details.
#[test]
fn view_missing_entry_returns_details() {
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
        .arg("view")
        .arg("missing-entry-id")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "ENTRY_NOT_FOUND");
    let details = extract_error_details(&output);
    assert_eq!(details["resource"], "entry");
    assert_eq!(details["entry_id"], "missing-entry-id");
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
        .arg(entry_ids[0].clone())
        .arg(entry_ids[1].clone())
        .assert()
        .success();

    let unread_after_read = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread_after_read["total_count"], 0);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("unread")
        .arg(entry_ids[0].clone())
        .assert()
        .success();

    let unread_after_unread = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread_after_unread["total_count"], 1);

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("tag")
        .arg(entry_ids[0].clone())
        .arg("--add")
        .arg("foo,bar")
        .arg("--remove")
        .arg("tech")
        .assert()
        .success();

    let foo_data = list_query_json(&paths.config_path, &paths.db_path, "tag:foo");
    assert_eq!(foo_data["total_count"], 1);

    let tech_data = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    assert_eq!(tech_data["total_count"], 1);
}

/// Ensures mark read fails when any entry id is missing and leaves state unchanged.
#[test]
fn mark_read_fails_when_any_entry_is_missing() {
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
    assert_eq!(unread_data["total_count"], 2);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("read")
        .arg(entry_ids[0].clone())
        .arg("missing-entry-id")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "ENTRY_NOT_FOUND", false);
    let error = extract_error_payload(&output);
    assert_eq!(error["message"], "some entries not found");
    assert!(error["details"].is_null(), "expected details=null");

    let unread_after = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread_after["total_count"], 2);
}

/// Ensures mark unread fails when any entry id is missing and leaves state unchanged.
#[test]
fn mark_unread_fails_when_any_entry_is_missing() {
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
        .arg(entry_ids[0].clone())
        .arg(entry_ids[1].clone())
        .assert()
        .success();
    let unread_after_read = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread_after_read["total_count"], 0);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("unread")
        .arg(entry_ids[0].clone())
        .arg("missing-entry-id")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "ENTRY_NOT_FOUND", false);
    let error = extract_error_payload(&output);
    assert_eq!(error["message"], "some entries not found");
    assert!(error["details"].is_null(), "expected details=null");

    let unread_after = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread_after["total_count"], 0);
}

/// Ensures mark tag --add fails when any entry id is missing and leaves state unchanged.
#[test]
fn mark_tag_add_fails_when_any_entry_is_missing() {
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
    let foo_before = list_query_json(&paths.config_path, &paths.db_path, "tag:foo");
    assert_eq!(foo_before["total_count"], 0);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("tag")
        .arg(entry_ids[0].clone())
        .arg("missing-entry-id")
        .arg("--add")
        .arg("foo")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "ENTRY_NOT_FOUND", false);
    let error = extract_error_payload(&output);
    assert_eq!(error["message"], "some entries not found");
    assert!(error["details"].is_null(), "expected details=null");

    let foo_after = list_query_json(&paths.config_path, &paths.db_path, "tag:foo");
    assert_eq!(foo_after["total_count"], 0);
}

/// Ensures mark tag --remove fails when any entry id is missing and leaves state unchanged.
#[test]
fn mark_tag_remove_fails_when_any_entry_is_missing() {
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
    let tech_before = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    assert_eq!(tech_before["total_count"], 2);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("tag")
        .arg(entry_ids[0].clone())
        .arg("missing-entry-id")
        .arg("--remove")
        .arg("tech")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "ENTRY_NOT_FOUND", false);
    let error = extract_error_payload(&output);
    assert_eq!(error["message"], "some entries not found");
    assert!(error["details"].is_null(), "expected details=null");

    let tech_after = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    assert_eq!(tech_after["total_count"], 2);
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

/// Runs `status` in JSON mode and returns its `data` object.
fn status_json(config_path: &str, db_path: &str) -> serde_json::Value {
    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path)
        .arg("--storage-root")
        .arg(db_root(db_path))
        .arg("status")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    extract_ok_data(&output)
}

/// Resolves an entry id from a title query.
fn entry_id_by_title(config_path: &str, db_path: &str, title: &str) -> String {
    let data = list_query_json(config_path, db_path, &format!("title:\"{title}\""));
    let items = data["items"].as_array().expect("items array");
    items
        .first()
        .and_then(|item| item["entry_id"].as_str())
        .map(|value| value.to_string())
        .expect("entry id by title")
}

/// Resolves the first feed id from list output.
fn first_feed_id_from_list(config_path: &str, db_path: &str) -> String {
    let data = list_query_json(config_path, db_path, "unread");
    let items = data["items"].as_array().expect("items array");
    items
        .first()
        .and_then(|item| item["feed_id"].as_str())
        .map(|value| value.to_string())
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
fn collect_item_ids(data: &serde_json::Value) -> Vec<String> {
    data["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| {
            item["entry_id"]
                .as_str()
                .expect("item entry_id")
                .to_string()
        })
        .collect()
}

/// Builds a simple RSS feed body with one item.
fn sample_feed_xml(guid: &str, title: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Example Feed</title>
    <link>https://example.com</link>
    <description>Example Feed</description>
    <item>
      <title>{title}</title>
      <link>https://example.com/{guid}</link>
      <guid>{guid}</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <description>Hello world</description>
    </item>
  </channel>
</rss>
"#
    )
}

/// Runs picofeedr with stdout closed to simulate downstream early exit.
fn run_with_closed_stdout(args: Vec<String>) -> Output {
    let bin = cargo_bin_cmd!("picofeedr").get_program().to_owned();
    let mut command = ProcessCommand::new(bin);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn picofeedr");
    drop(child.stdout.take());
    child.wait_with_output().expect("wait for picofeedr")
}
