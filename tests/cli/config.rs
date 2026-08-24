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
    let mut cmd = fixture_cmd_json(config_path, db_path);
    cmd.arg("sync").arg("--check");
    cmd
}

fn sync_check_plain_cmd(config_path: &str, db_path: &str) -> assert_cmd::Command {
    let mut cmd = fixture_cmd_plain(config_path, db_path);
    cmd.arg("sync").arg("--check");
    cmd
}

fn feeds_json_cmd(config_path: &str, db_path: &str) -> assert_cmd::Command {
    let mut cmd = fixture_cmd_json(config_path, db_path);
    cmd.arg("feeds");
    cmd
}

fn sync_json_cmd(config_path: &str, db_path: &str) -> assert_cmd::Command {
    let mut cmd = fixture_cmd_json(config_path, db_path);
    cmd.arg("sync");
    cmd
}

fn write_feeds_case(temp: &TempDir, feeds: &str) -> FixturePaths {
    let paths = write_fixture_files(temp);
    fs::write(&paths.feeds_path, feeds).expect("rewrite feeds");
    paths
}

fn sync_check_errors(config_path: &str, db_path: &str) -> Vec<serde_json::Value> {
    let output = sync_check_json_cmd(config_path, db_path)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let data = extract_result(&output, "warning");
    assert_eq!(data["valid"], false);
    data["errors"].as_array().expect("errors array").clone()
}

fn assert_sync_check_has_issue(
    config_path: &str,
    db_path: &str,
    expected_code: &str,
    expected_path: &str,
) {
    let errors = sync_check_errors(config_path, db_path);
    assert!(
        errors.iter().any(|issue| issue["code"] == expected_code),
        "expected {expected_code} error"
    );
    assert!(
        errors.iter().any(|issue| issue["path"] == expected_path),
        "expected issue path {expected_path}"
    );
}

fn assert_sync_check_type_error(feeds: &str, expected_message: &str) {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_feeds_case(&temp, feeds);
    let output = sync_check_json_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "CONFIG_ERROR");
    assert!(
        error["message"]
            .as_str()
            .expect("error message")
            .contains(expected_message)
    );
}

fn assert_runtime_validation_failure(
    config_path: &str,
    db_path: &str,
    expected_code: &str,
    expected_path: &str,
) {
    let output = sync_json_cmd(config_path, db_path)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "CONFIG_ERROR", false);
    let details = extract_error_details(&output);
    assert_eq!(details["first_issue_code"], expected_code);
    assert_eq!(details["first_issue_path"], expected_path);
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

    let data = extract_result(&output, "ok");
    assert_eq!(data["valid"], true);
    assert!(data["errors"].as_array().expect("errors").is_empty());
    assert!(data["warnings"].as_array().expect("warnings").is_empty());
    assert_eq!(data["checked_feeds"], 1);
}

#[test]
fn config_check_reports_skipped_feeds() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feeds = r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/active.xml
        title: Active
      - url: https://example.com/skipped.xml
        title: Skipped
        skip: true
"#;
    fs::write(&paths.feeds_path, feeds).expect("rewrite feeds");

    let output = sync_check_json_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_result(&output, "ok");
    assert_eq!(data["valid"], true);
    assert_eq!(data["checked_feeds"], 2);
    assert_eq!(data["skipped_feeds"], 1);
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

    let data = extract_result(&output, "ok");
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

    let data = extract_result(&output, "ok");
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

    let data = extract_result(&output, "warning");
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

    let data = extract_result(&output, "warning");
    assert_eq!(data["valid"], false);
    let errors = data["errors"].as_array().expect("errors array");
    assert!(
        errors.iter().any(|issue| issue["code"] == "EMPTY_FEED_URL"),
        "expected EMPTY_FEED_URL error"
    );
}

#[test]
fn config_check_fails_on_blank_feed_tag() {
    let temp = TempDir::new().expect("tempdir");
    let feeds = r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
        tags: ["   "]
"#;
    let paths = write_feeds_case(&temp, feeds);

    assert_sync_check_has_issue(
        &paths.config_path,
        &paths.db_path,
        "EMPTY_TAG_NAME",
        "picofeedr.group.feeds[0].tags",
    );
}

#[test]
fn config_check_fails_on_reserved_comma_in_feed_tag() {
    let temp = TempDir::new().expect("tempdir");
    let feeds = r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
        tags: ["rust,cli"]
"#;
    let paths = write_feeds_case(&temp, feeds);

    assert_sync_check_has_issue(
        &paths.config_path,
        &paths.db_path,
        "INVALID_TAG_NAME",
        "picofeedr.group.feeds[0].tags",
    );
}

#[test]
fn config_check_fails_on_control_character_in_feed_tag() {
    let temp = TempDir::new().expect("tempdir");
    let feeds = r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
        tags: ["line\nbreak"]
"#;
    let paths = write_feeds_case(&temp, feeds);

    assert_sync_check_has_issue(
        &paths.config_path,
        &paths.db_path,
        "INVALID_TAG_NAME",
        "picofeedr.group.feeds[0].tags",
    );
}

#[test]
fn config_check_fails_on_feed_tag_over_64_unicode_characters() {
    let temp = TempDir::new().expect("tempdir");
    let tag = "技".repeat(65);
    let feeds = format!(
        r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
        tags: ["{tag}"]
"#
    );
    let paths = write_feeds_case(&temp, &feeds);

    assert_sync_check_has_issue(
        &paths.config_path,
        &paths.db_path,
        "INVALID_TAG_NAME",
        "picofeedr.group.feeds[0].tags",
    );
}

#[test]
fn config_check_accepts_unicode_feed_tags() {
    let temp = TempDir::new().expect("tempdir");
    let max_length_tag = "技".repeat(64);
    let feeds = format!(
        r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
        tags: [日本語, "機械 学習", "rust🦀", "分類/開発", "a|b", "{max_length_tag}"]
"#
    );
    let paths = write_feeds_case(&temp, &feeds);

    sync_check_json_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .success();
}

#[test]
fn config_check_fails_on_blank_auto_tag_value() {
    let temp = TempDir::new().expect("tempdir");
    let feeds = r#"picofeedr:
  group:
    auto_tags:
      - title_contains: [Steam]
        add_tags: [" "]
"#;
    let paths = write_feeds_case(&temp, feeds);

    assert_sync_check_has_issue(
        &paths.config_path,
        &paths.db_path,
        "EMPTY_TAG_NAME",
        "picofeedr.group.auto_tags[0].add_tags",
    );
}

#[test]
fn config_check_fails_on_reserved_comma_in_auto_tag_value() {
    let temp = TempDir::new().expect("tempdir");
    let feeds = r#"picofeedr:
  group:
    auto_tags:
      - title_contains: [Steam]
        add_tags: ["game,news"]
"#;
    let paths = write_feeds_case(&temp, feeds);

    assert_sync_check_has_issue(
        &paths.config_path,
        &paths.db_path,
        "INVALID_TAG_NAME",
        "picofeedr.group.auto_tags[0].add_tags",
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

    let data = extract_result(&output, "warning");
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

    let data = extract_result(&output, "ok");
    assert_eq!(data["valid"], true);
    assert_eq!(data["checked_feeds"], 1);
}

#[test]
fn config_check_ignores_unknown_feeds_yaml_top_level_keys() {
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

    let data = extract_result(&output, "ok");
    assert_eq!(data["valid"], true);
    assert!(data["errors"].as_array().expect("errors").is_empty());
    assert!(data["warnings"].as_array().expect("warnings").is_empty());
}

#[test]
fn config_rejects_unknown_keys_at_all_levels() {
    let cases = [
        (None, "unknown_top_level"),
        (Some("feeds"), "unknown_feeds"),
        (Some("sync"), "unknown_sync"),
        (Some("storage"), "unknown_storage"),
        (Some("query"), "unknown_query"),
        (Some("cli"), "unknown_cli"),
    ];

    for (section, field) in cases {
        let temp = TempDir::new().expect("tempdir");
        let config_path = temp.path().join("config.toml");
        let root_dir = temp.path().display();
        let config = match section {
            None => format!("{field} = true\n\n[storage]\nroot_dir = \"{root_dir}\""),
            Some("storage") => {
                format!("[storage]\nroot_dir = \"{root_dir}\"\n{field} = true")
            }
            Some(section) => {
                format!("[storage]\nroot_dir = \"{root_dir}\"\n\n[{section}]\n{field} = true")
            }
        };
        fs::write(&config_path, config).expect("write config");

        let output = picofeedr_cmd_json()
            .arg("--config")
            .arg(&config_path)
            .arg("tags")
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();

        assert_error_envelope(&output, "CONFIG_ERROR", false);
        assert!(
            extract_error_payload(&output)["message"]
                .as_str()
                .is_some_and(|message| message.contains(field)),
            "expected unknown field {field}"
        );
    }
}

#[test]
fn config_sync_parallel_enforces_range() {
    for (parallel, should_succeed) in [(0, false), (1, true), (64, true), (65, false)] {
        let temp = TempDir::new().expect("tempdir");
        let config_path = temp.path().join("config.toml");
        let config = format!(
            "[sync]\nparallel = {parallel}\n\n[storage]\nroot_dir = \"{}\"\n",
            temp.path().display()
        );
        fs::write(&config_path, config).expect("write config");

        let mut cmd = picofeedr_cmd_json();
        cmd.arg("--config").arg(&config_path).arg("tags");
        if should_succeed {
            cmd.assert().success();
        } else {
            let output = cmd.assert().failure().get_output().stdout.clone();
            assert_error_envelope(&output, "CONFIG_ERROR", false);
            let details = extract_error_details(&output);
            assert_eq!(details["path"], "sync.parallel");
        }
    }
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

    let stderr = fixture_cmd_plain(&paths.config_path, &paths.db_path)
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
fn feeds_ignores_blocking_feeds_yaml_validation() {
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
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_result(&output, "ok");
    assert!(data["feeds"].as_array().expect("feeds array").is_empty());
}

#[test]
fn sync_check_rejects_invalid_yaml_field_types() {
    let cases = [
        (
            r#"picofeedr:
  group:
    tags: "not-a-list"
    feeds:
      - url: https://example.com/feed
        title: Example
"#,
            "feed group tags must be a list at picofeedr.group.tags",
        ),
        (
            r#"picofeedr:
  group:
    tags: null
    feeds:
      - url: https://example.com/feed
        title: Example
"#,
            "feed group tags must be a list at picofeedr.group.tags",
        ),
        (
            r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: 42
"#,
            "feed entry title must be a string at picofeedr.group.feeds[0].title",
        ),
        (
            r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: null
"#,
            "feed entry title must be a string at picofeedr.group.feeds[0].title",
        ),
        (
            r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
        tags: "not-a-list"
"#,
            "feed entry tags must be a list at picofeedr.group.feeds[0].tags",
        ),
        (
            r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
        tags: null
"#,
            "feed entry tags must be a list at picofeedr.group.feeds[0].tags",
        ),
        (
            r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
        skip: "true"
"#,
            "feed entry skip must be a boolean at picofeedr.group.feeds[0].skip",
        ),
    ];

    for (feeds, expected_message) in cases {
        assert_sync_check_type_error(feeds, expected_message);
    }
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
fn sync_fails_fast_on_blank_feed_tag() {
    let temp = TempDir::new().expect("tempdir");
    let feeds = r#"picofeedr:
  group:
    feeds:
      - url: https://example.com/feed
        title: Example
        tags: ["   "]
"#;
    let paths = write_feeds_case(&temp, feeds);

    assert_runtime_validation_failure(
        &paths.config_path,
        &paths.db_path,
        "EMPTY_TAG_NAME",
        "picofeedr.group.feeds[0].tags",
    );
}

#[test]
fn sync_fails_fast_on_blank_auto_tag_value() {
    let temp = TempDir::new().expect("tempdir");
    let feeds = r#"picofeedr:
  group:
    auto_tags:
      - title_contains: [Steam]
        add_tags: [" "]
"#;
    let paths = write_feeds_case(&temp, feeds);

    assert_runtime_validation_failure(
        &paths.config_path,
        &paths.db_path,
        "EMPTY_TAG_NAME",
        "picofeedr.group.auto_tags[0].add_tags",
    );
}

#[test]
fn sync_validation_error_includes_summary_details() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let feeds = r#"picofeedr:
  group:
    feeds:
      - url: ""
        title: Empty URL
"#;
    fs::write(&paths.feeds_path, feeds).expect("rewrite feeds");

    let output = sync_json_cmd(&paths.config_path, &paths.db_path)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "CONFIG_ERROR", false);
    let details = extract_error_details(&output);
    assert_eq!(details["hint"], "run_sync_check");
    assert!(
        details["error_count"]
            .as_u64()
            .is_some_and(|count| count >= 1)
    );
    assert!(details["first_issue_code"].as_str().is_some());
    assert!(details["first_issue_path"].as_str().is_some());
}

#[test]
fn sync_allows_warning_only_config() {
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

    let output = fixture_cmd_json(
        &inputs.config_path,
        inputs.db_path.as_deref().expect("db path"),
    )
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
    let paths = SyncFixtureBuilder::new(&temp)
        .unread_tag("fresh")
        .build_db();

    fixture_cmd_json(&paths.config_path, &paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let output = fixture_cmd_json(&paths.config_path, &paths.db_path)
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

    let data = extract_result(&output, "ok");
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
fn unread_tag_is_trimmed_before_use() {
    let temp = TempDir::new().expect("tempdir");
    let paths = SyncFixtureBuilder::new(&temp)
        .unread_tag(" fresh ")
        .build_db();

    fixture_cmd_json(&paths.config_path, &paths.db_path)
        .arg("sync")
        .assert()
        .success();

    let output = fixture_cmd_json(&paths.config_path, &paths.db_path)
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

    let data = extract_result(&output, "ok");
    let items = data["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2);
    for item in items {
        let tags = item["tags"].as_array().expect("tags array");
        let tag_values: Vec<String> = tags
            .iter()
            .map(|tag| tag.as_str().unwrap().to_string())
            .collect();
        assert!(tag_values.contains(&"fresh".to_string()));
        assert!(!tag_values.contains(&" fresh ".to_string()));
    }
}

#[test]
fn unread_query_uses_unread_tag_alias_when_unread_management_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_synced_fixture_with_unread_management_disabled(&temp);

    let unread = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread["total_count"], 0);

    let items = unread["items"].as_array().expect("items array");
    assert!(items.is_empty());
}

#[test]
fn tags_command_does_not_create_empty_tag_when_unread_management_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_synced_fixture_with_unread_management_disabled(&temp);

    let output = fixture_cmd_json(&paths.config_path, &paths.db_path)
        .arg("tags")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_result(&output, "ok");
    let tags = data["tags"].as_array().expect("tags array");
    assert!(
        !tags
            .iter()
            .any(|tag| tag.as_str().is_some_and(str::is_empty))
    );
}

#[test]
fn blank_unread_tag_is_rejected_even_when_unread_management_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let paths = SyncFixtureBuilder::new(&temp)
        .manage_unread(false)
        .unread_tag("")
        .build_db();

    let output = fixture_cmd_json(&paths.config_path, &paths.db_path)
        .arg("tags")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "CONFIG_ERROR", false);
    let details = extract_error_details(&output);
    assert_eq!(details["path"], "unread_tag");
}

#[test]
fn reserved_comma_in_unread_tag_is_rejected() {
    let temp = TempDir::new().expect("tempdir");
    let paths = SyncFixtureBuilder::new(&temp)
        .unread_tag("fresh,unread")
        .build_db();

    let output = fixture_cmd_json(&paths.config_path, &paths.db_path)
        .arg("tags")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "CONFIG_ERROR", false);
    let details = extract_error_details(&output);
    assert_eq!(details["path"], "unread_tag");
    assert_eq!(details["hint"], "remove_reserved_comma");
}

#[test]
fn overlong_unread_tag_is_rejected() {
    let temp = TempDir::new().expect("tempdir");
    let tag = "技".repeat(65);
    let paths = SyncFixtureBuilder::new(&temp).unread_tag(&tag).build_db();

    let output = fixture_cmd_json(&paths.config_path, &paths.db_path)
        .arg("tags")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    assert_error_envelope(&output, "CONFIG_ERROR", false);
    let details = extract_error_details(&output);
    assert_eq!(details["path"], "unread_tag");
    assert_eq!(details["hint"], "shorten_tag_name");
}

#[test]
fn fatal_config_rejects_zero_default_limit() {
    let temp = TempDir::new().expect("tempdir");
    let paths = SyncFixtureBuilder::new(&temp).query_limits(0, 5).build_db();

    let output = fixture_cmd_json(&paths.config_path, &paths.db_path)
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
    let paths = SyncFixtureBuilder::new(&temp).query_limits(1, 0).build_db();

    let output = fixture_cmd_json(&paths.config_path, &paths.db_path)
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
    let paths = SyncFixtureBuilder::new(&temp).query_limits(6, 5).build_db();

    let output = fixture_cmd_json(&paths.config_path, &paths.db_path)
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
