use super::*;

fn spawn_gopher_feed_server(body: &'static [u8]) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut buf = [0_u8; 64];
        loop {
            let read = stream.read(&mut buf).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
            if request.ends_with(b"\r\n") {
                break;
            }
        }
        assert_eq!(request, b"feed.xml\r\n");
        stream.write_all(body).expect("write response");
    });
    (format!("gopher://{addr}/0feed.xml"), server)
}

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
    assert!(!output_str.contains("sync:feed start"));
    assert!(output_str.contains("sync:feed-ok index=1/1"));
    assert!(output_str.contains("url=file://"));
    assert!(output_str.contains("entries=2"));
    assert!(output_str.contains("sync:done status=completed"));
}

#[test]
fn sync_plain_summary_uses_single_log_line() {
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

    let output = String::from_utf8_lossy(&output);
    assert!(output.contains("sync:done status=completed"));
    assert!(output.contains("fetched_feed_count=1"));
    assert!(output.contains("failed_feed_count=0"));
    assert!(output.contains("new_entry_count=2"));
    assert!(output.contains("duration_ms="));
    assert!(output.contains("errors=0"));
}

#[test]
fn sync_ingests_entries_from_gopher_feed() {
    let temp = TempDir::new().expect("tempdir");
    let feeds_path = temp.path().join("feeds.yaml");
    let config_path = temp.path().join("config.toml");
    let db_path = temp.path().join("db.sqlite");

    let (feed_url, server_thread) = spawn_gopher_feed_server(
        br#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Example Feed</title>
    <link>https://example.com</link>
    <description>Example Feed</description>
    <item>
      <guid>gopher-1</guid>
      <title>From Gopher</title>
      <link>https://example.com/gopher-1</link>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <description>Entry from gopher</description>
    </item>
  </channel>
</rss>
.
"#,
    );
    let feeds = format!(
        "picofeedr:\n  tech:\n    tags: [tech]\n    feeds:\n      - url: {feed_url}\n        title: Gopher Feed\n"
    );
    fs::write(&feeds_path, feeds).expect("write feeds");
    let config = format!(
        "unread_tag = \"unread\"\n\n[feeds]\nsource = \"{}\"\n\n[storage]\nroot_dir = \"{}\"\n\n[sync]\ntimeout = 1\nretry_count = 0\nretry_delay = 0\n",
        feeds_path.display(),
        temp.path().display()
    );
    fs::write(&config_path, config).expect("write config");

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path.display().to_string())
        .arg("--storage-root")
        .arg(db_root(db_path.to_str().expect("db path")))
        .arg("sync")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    server_thread.join().expect("join gopher server thread");
    let data = extract_ok_data(&output);
    assert_eq!(data["status"], "completed");
    assert_eq!(data["failed_feed_count"], 0);
    assert_eq!(data["new_entry_count"], 1);
}

#[test]
fn sync_reports_parse_error_for_gopher_directory_listing() {
    let temp = TempDir::new().expect("tempdir");
    let feeds_path = temp.path().join("feeds.yaml");
    let config_path = temp.path().join("config.toml");
    let db_path = temp.path().join("db.sqlite");

    let (feed_url, server_thread) =
        spawn_gopher_feed_server(b"0About\tselector\texample.com\t70\r\n.\r\n");
    let feeds = format!(
        "picofeedr:\n  tech:\n    tags: [tech]\n    feeds:\n      - url: {feed_url}\n        title: Gopher Menu\n"
    );
    fs::write(&feeds_path, feeds).expect("write feeds");
    let config = format!(
        "unread_tag = \"unread\"\n\n[feeds]\nsource = \"{}\"\n\n[storage]\nroot_dir = \"{}\"\n\n[sync]\ntimeout = 1\nretry_count = 0\nretry_delay = 0\n",
        feeds_path.display(),
        temp.path().display()
    );
    fs::write(&config_path, config).expect("write config");

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path.display().to_string())
        .arg("--storage-root")
        .arg(db_root(db_path.to_str().expect("db path")))
        .arg("sync")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    server_thread.join().expect("join gopher server thread");
    let data = extract_ok_data(&output);
    let errors = data["errors"].as_array().expect("errors");
    assert_eq!(errors[0]["code"], "PARSE_FAILED");
    assert_eq!(errors[0]["retryable"], false);
}

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
}

#[test]
fn sync_check_reports_invalid_nested_auto_tag_rule_path() {
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
        .arg("sync")
        .arg("--check")
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
            .any(|issue| issue["path"] == "picofeedr.group.auto_tags[0].add_tags")
    );
}

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
}

#[test]
fn sync_creates_missing_storage_root_for_override() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files_fs(&temp);
    let override_root = temp.path().join("missing").join("override-root");
    assert!(!override_root.exists());

    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(&override_root)
        .arg("sync")
        .assert()
        .success();

    let override_db_path = override_root.join("db.sqlite");
    assert!(override_root.is_dir());
    assert!(override_db_path.exists());
}

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
}

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
        .clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("sync:done status=partial_failed"));
    assert!(stderr.contains("sync:feed-error"));
    assert!(stderr.contains("index=2/2"));
}

#[test]
fn sync_plain_reports_error_details_as_log_lines() {
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
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("sync:done"));
    assert!(stdout.contains("errors=1"));
    assert!(stderr.contains("sync:feed-error"));
    assert!(stderr.contains("index=2/2"));
    assert!(stderr.contains("code=PARSE_FAILED"));
    assert!(stderr.contains("retryable=false"));
    assert!(stderr.contains("message="));
}

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
}

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
        .clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("sync:done status=failed"));
    assert!(stderr.contains("sync:feed-error"));
    assert!(stderr.contains("index="));
}

#[test]
fn sync_http_404_fetch_failed_is_not_retryable() {
    let temp = TempDir::new().expect("tempdir");
    let feeds_path = temp.path().join("feeds.yaml");
    let config_path = temp.path().join("config.toml");
    let db_path = temp.path().join("db.sqlite");

    let (feed_url, server_thread) = spawn_http_404_feed_server();
    let feeds = format!(
        "picofeedr:\n  tech:\n    tags: [tech]\n    feeds:\n      - url: {feed_url}\n        title: Missing Feed\n"
    );
    fs::write(&feeds_path, feeds).expect("write feeds");
    let config = format!(
        "unread_tag = \"unread\"\n\n[feeds]\nsource = \"{}\"\n\n[storage]\nroot_dir = \"{}\"\n\n[sync]\ntimeout = 1\nretry_count = 0\nretry_delay = 0\n",
        feeds_path.display(),
        temp.path().display()
    );
    fs::write(&config_path, config).expect("write config");

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path.display().to_string())
        .arg("--storage-root")
        .arg(db_root(db_path.to_str().expect("db path")))
        .arg("sync")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    server_thread.join().expect("join 404 server thread");
    let data = extract_ok_data(&output);
    let errors = data["errors"].as_array().expect("errors");
    assert_eq!(errors[0]["retryable"], false);
}

#[test]
fn sync_plain_http_404_error_output_is_not_redundant() {
    let temp = TempDir::new().expect("tempdir");
    let feeds_path = temp.path().join("feeds.yaml");
    let config_path = temp.path().join("config.toml");
    let db_path = temp.path().join("db.sqlite");

    let (feed_url, server_thread) = spawn_http_404_feed_server();
    let feeds = format!(
        "picofeedr:\n  tech:\n    tags: [tech]\n    feeds:\n      - url: {feed_url}\n        title: Missing Feed\n"
    );
    fs::write(&feeds_path, feeds).expect("write feeds");
    let config = format!(
        "unread_tag = \"unread\"\n\n[feeds]\nsource = \"{}\"\n\n[storage]\nroot_dir = \"{}\"\n\n[sync]\ntimeout = 1\nretry_count = 0\nretry_delay = 0\n",
        feeds_path.display(),
        temp.path().display()
    );
    fs::write(&config_path, config).expect("write config");

    let output = picofeedr_cmd_plain()
        .arg("--config")
        .arg(config_path.display().to_string())
        .arg("--storage-root")
        .arg(db_root(db_path.to_str().expect("db path")))
        .arg("sync")
        .assert()
        .success()
        .get_output()
        .clone();

    server_thread.join().expect("join 404 server thread");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("sync:feed-error"));
    assert!(stderr.contains("index=1/1"));
    assert!(stderr.contains("code=FETCH_FAILED"));
    assert!(stderr.contains("retryable=false"));
}

#[test]
fn sync_rejects_oversized_feed_body() {
    let temp = TempDir::new().expect("tempdir");
    let feeds_path = temp.path().join("feeds.yaml");
    let config_path = temp.path().join("config.toml");
    let db_path = temp.path().join("db.sqlite");
    let feed_path = temp.path().join("feed.xml");
    let feed_url = format!("file://{}", feed_path.display());

    fs::write(&feed_path, sample_feed_xml("entry-1", "Entry")).expect("write feed");
    let feeds = format!(
        "picofeedr:\n  tech:\n    tags: [tech]\n    feeds:\n      - url: {feed_url}\n        title: Big Feed\n"
    );
    fs::write(&feeds_path, feeds).expect("write feeds");
    let config = format!(
        "unread_tag = \"unread\"\n\n[feeds]\nsource = \"{}\"\n\n[storage]\nroot_dir = \"{}\"\n\n[sync]\ntimeout = 1\nretry_count = 0\nretry_delay = 0\nmax_feed_bytes = 32\n",
        feeds_path.display(),
        temp.path().display()
    );
    fs::write(&config_path, config).expect("write config");

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(config_path.display().to_string())
        .arg("--storage-root")
        .arg(db_root(db_path.to_str().expect("db path")))
        .arg("sync")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data = extract_ok_data(&output);
    let errors = data["errors"].as_array().expect("errors");
    assert_eq!(errors[0]["code"], "FETCH_FAILED");
}
