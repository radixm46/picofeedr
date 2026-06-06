use super::*;

#[test]
fn feeds_reads_db_rows_without_validating_feeds_yaml() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    sync_fixture_ok(&paths);
    fs::write(temp.path().join("feeds.yaml"), "picofeedr: [").expect("break feeds yaml");

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
    assert_eq!(feeds[0]["title"], "Example Feed");
    assert_eq!(
        feeds[0]["url"],
        format!("file://{}", temp.path().join("feed.xml").display())
    );
    assert!(feeds[0].get("tags").is_none());
}

#[test]
fn tags_command_returns_tag_dictionary() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    sync_fixture_ok(&paths);

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
    assert_eq!(tag_values, vec!["hot", "tech", "unread"]);
}

#[test]
fn feeds_plain_outputs_tsv_columns() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    sync_fixture_ok(&paths);

    let output = picofeedr_cmd_plain()
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

    let output = String::from_utf8(output).expect("utf8");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 1);
    let columns: Vec<&str> = lines[0].split('\t').collect();
    assert_eq!(columns.len(), 4);
    assert_eq!(columns[0], "Example Feed");
    assert_eq!(
        columns[1],
        format!("file://{}", temp.path().join("feed.xml").display())
    );
    assert_eq!(columns[2], "https://example.com/");
    assert_eq!(columns[3], "");
}

#[test]
fn feeds_plain_with_id_appends_feed_id_column() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    sync_fixture_ok(&paths);

    let output = picofeedr_cmd_plain()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("feeds")
        .arg("--id")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let output = String::from_utf8(output).expect("utf8");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 1);
    let columns: Vec<&str> = lines[0].split('\t').collect();
    assert_eq!(columns.len(), 5);
    assert!(!columns[4].is_empty());
}

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

#[test]
fn status_does_not_track_feeds_read_revision() {
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
    assert_eq!(status["revision"], 0);
    assert!(status["last_write_at"].is_null());
}

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

#[test]
fn status_plain_renders_human_readable_local_timestamps() {
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
        .arg("status")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output_str = String::from_utf8(output).expect("plain status utf-8");
    let last_write_at = status_plain_field(&output_str, "last_write_at");
    let last_sync_at = status_plain_field(&output_str, "last_sync_at");

    assert!(looks_like_human_datetime(last_write_at));
    assert!(looks_like_human_datetime(last_sync_at));
}
