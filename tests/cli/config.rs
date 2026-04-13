use super::*;
use std::path::PathBuf;

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

fn sync_check_json_cmd(config_path: &str, db_path: &str) -> assert_cmd::Command {
    let mut cmd = picofeedr_cmd_json();
    cmd.arg("--config")
        .arg(config_path)
        .arg("--storage-root")
        .arg(db_root(db_path))
        .arg("sync")
        .arg("--check");
    cmd
}

fn sync_check_plain_cmd(config_path: &str, db_path: &str) -> assert_cmd::Command {
    let mut cmd = picofeedr_cmd_plain();
    cmd.arg("--config")
        .arg(config_path)
        .arg("--storage-root")
        .arg(db_root(db_path))
        .arg("sync")
        .arg("--check");
    cmd
}

fn feeds_json_cmd(config_path: &str, db_path: &str) -> assert_cmd::Command {
    let mut cmd = picofeedr_cmd_json();
    cmd.arg("--config")
        .arg(config_path)
        .arg("--storage-root")
        .arg(db_root(db_path))
        .arg("feeds");
    cmd
}

fn sync_json_cmd(config_path: &str, db_path: &str) -> assert_cmd::Command {
    let mut cmd = picofeedr_cmd_json();
    cmd.arg("--config")
        .arg(config_path)
        .arg("--storage-root")
        .arg(db_root(db_path))
        .arg("sync");
    cmd
}

fn write_default_home_feeds(temp: &TempDir) -> (PathBuf, PathBuf, PathBuf) {
    let home = temp.path().to_path_buf();
    let config_dir = home.join(".config").join("picofeedr");
    fs::create_dir_all(&config_dir).expect("create config dir");

    let feed_path = home.join("sample-feed.xml");
    fs::write(
        &feed_path,
        sample_feed_xml("default-home", "Default Home Feed"),
    )
    .expect("write feed");

    let feeds_path = config_dir.join("feeds.yaml");
    let feeds = format!(
        r#"picofeedr:
  group:
    feeds:
      - url: file://{}
        title: Default Home Feed
"#,
        feed_path.display()
    );
    fs::write(&feeds_path, feeds).expect("write feeds");

    let storage_root = home.join(".local").join("share").join("picofeedr");
    (home, feeds_path, storage_root)
}

#[test]
fn config_check_returns_validation_report() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let output = sync_check_json_cmd(&paths.config_path, &paths.db_path)
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

#[test]
fn config_check_uses_default_paths_without_config_file() {
    let temp = TempDir::new().expect("tempdir");
    let (home, feeds_path, storage_root) = write_default_home_feeds(&temp);
    assert!(feeds_path.exists());
    assert!(
        !home
            .join(".config")
            .join("picofeedr")
            .join("config.toml")
            .exists()
    );
    assert!(!storage_root.exists());

    let output = picofeedr_cmd_json()
        .env("HOME", &home)
        .arg("sync")
        .arg("--check")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["valid"], true);
    assert_eq!(data["checked_feeds"], 1);
    assert!(!storage_root.exists());
}

#[test]
fn sync_uses_default_paths_without_config_file() {
    let temp = TempDir::new().expect("tempdir");
    let (home, feeds_path, storage_root) = write_default_home_feeds(&temp);
    assert!(feeds_path.exists());
    assert!(
        !home
            .join(".config")
            .join("picofeedr")
            .join("config.toml")
            .exists()
    );

    let output = picofeedr_cmd_json()
        .env("HOME", &home)
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
    assert_eq!(data["new_entry_count"], 1);
    assert!(storage_root.join("db.sqlite").exists());
}

#[test]
fn explicit_missing_config_path_still_fails() {
    let temp = TempDir::new().expect("tempdir");
    let missing_config = temp.path().join("missing.toml");

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&missing_config)
        .arg("tags")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "CONFIG_ERROR", false);
    let details = extract_error_details(&output);
    assert_eq!(details["hint"], "failed_to_read_config");
}

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

    let output = sync_check_json_cmd(&paths.config_path, &paths.db_path)
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

    let output = sync_check_json_cmd(&paths.config_path, &paths.db_path)
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

#[test]
fn config_check_fails_on_invalid_title_regex() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feeds = r#"picofeedr:
  group:
    auto_tags:
      - title_regex: "("
        add_tags: [broken]
    feeds:
      - url: https://example.com/feed
        title: Example
"#;
    fs::write(&paths.feeds_path, feeds).expect("rewrite feeds");

    let output = sync_check_json_cmd(&paths.config_path, &paths.db_path)
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
            .any(|issue| issue["code"] == "INVALID_TITLE_REGEX"),
        "expected INVALID_TITLE_REGEX error"
    );
}

#[test]
fn config_check_does_not_require_db() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let output = sync_check_json_cmd(
        &paths.config_path,
        temp.path()
            .join("missing-dir")
            .join("db.sqlite")
            .to_str()
            .expect("db path"),
    )
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["valid"], true);
    assert_eq!(data["checked_feeds"], 1);
}

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

    let output = sync_check_json_cmd(&paths.config_path, &paths.db_path)
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

#[test]
fn sync_check_plain_shows_summary_and_diagnostic_lines() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feeds = r#"picofeedr:
  group:
    feeds:
      - url: ""
        title: Empty URL
"#;
    fs::write(&paths.feeds_path, feeds).expect("rewrite feeds");

    let output = sync_check_plain_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("valid: false"));
    assert!(output.contains("checked_feeds: 1"));
    assert!(output.contains("errors: 1"));
    assert!(output.contains("warnings: 0"));
    assert!(output.contains("error: code=EMPTY_FEED_URL path=picofeedr.group.feeds[0].url"));
}

#[test]
fn feeds_check_is_rejected() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);

    let stderr = picofeedr_cmd_plain()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("feeds")
        .arg("--check")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();

    let stderr = String::from_utf8(stderr).expect("utf8");
    assert!(stderr.contains("--check"));
}

#[test]
fn feeds_fails_fast_on_duplicate_url() {
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

    let output = feeds_json_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "CONFIG_ERROR", false);
}

#[test]
fn sync_fails_fast_on_duplicate_url() {
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

    let output = sync_json_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "CONFIG_ERROR", false);
}

#[test]
fn sync_fails_fast_on_invalid_title_regex() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feeds = r#"picofeedr:
  group:
    auto_tags:
      - title_regex: "("
        add_tags: [broken]
    feeds:
      - url: https://example.com/feed
        title: Example
"#;
    fs::write(&paths.feeds_path, feeds).expect("rewrite feeds");

    let output = sync_json_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "CONFIG_ERROR", false);
}

#[test]
fn feeds_and_sync_allow_warning_only_config() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feed_path = temp.path().join("warning-only.xml");
    fs::write(
        &feed_path,
        sample_feed_xml("warning-only", "Warning Only Feed"),
    )
    .expect("write feed");
    let feeds = format!(
        r#"picofeedr:
  group:
    feeds:
      - url: file://{}
        title: Example
        tags: [dup, dup]
"#,
        feed_path.display()
    );
    fs::write(&paths.feeds_path, feeds).expect("rewrite feeds");

    feeds_json_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .success();
    sync_json_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .success();
}

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

fn fatal_case_invalid_toml_syntax(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    fs::write(&config_path, "unread_tag = ").expect("write config");
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: None,
        command_args: vec!["tags"],
    }
}

fn fatal_case_missing_feeds_yaml(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("missing.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: Some(db_path.display().to_string()),
        command_args: vec!["sync", "--check"],
    }
}

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

fn fatal_case_invalid_feeds_yaml(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);
    fs::write(&feeds_path, "picofeedr: [").expect("write feeds");
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: Some(db_path.display().to_string()),
        command_args: vec!["sync", "--check"],
    }
}

fn fatal_case_missing_top_level_feeds_key(temp: &TempDir) -> FatalEnvelopeInputs {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);
    fs::write(&feeds_path, "auto_tags: []").expect("write feeds");
    FatalEnvelopeInputs {
        config_path: config_path.display().to_string(),
        db_path: Some(db_path.display().to_string()),
        command_args: vec!["sync", "--check"],
    }
}

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
