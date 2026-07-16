use super::*;

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

#[test]
fn view_plain_renders_kv_metadata_and_human_timestamps() {
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
        .arg(entry_id)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("entry_id: "));
    assert!(output.contains("title: First Entry"));
    assert!(output.contains("feed_title: Example Feed"));
    assert!(output.contains("feed_id: "));
    let published_at = status_plain_field(&output, "published_at");
    let first_seen_at = status_plain_field(&output, "first_seen_at");
    assert!(looks_like_human_datetime(published_at));
    assert!(looks_like_human_datetime(first_seen_at));
    assert!(output.contains("\n\nHello world\n"));
}

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
}

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

#[test]
fn mark_tag_rejects_tag_over_64_unicode_characters() {
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
    let entry_id = collect_item_ids(&unread_data).remove(0);
    let tag = "技".repeat(65);
    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("tag")
        .arg(entry_id)
        .arg("--add")
        .arg(&tag)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "INVALID_QUERY");
    let details = extract_error_details(&output);
    assert_eq!(details["kind"], "invalid_tag_name");
    assert_eq!(details["field"], "tag");
    assert_eq!(details["value"], tag);
    assert_eq!(details["hint"], "shorten_tag_name");
}

#[test]
fn mark_tag_accepts_unicode_tag_names() {
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
    let entry_id = collect_item_ids(&unread_data).remove(0);
    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("tag")
        .arg(entry_id)
        .arg("--add")
        .arg("日本語 ニュース,rust🦀")
        .assert()
        .success();

    let cjk = list_query_json(
        &paths.config_path,
        &paths.db_path,
        r#"tag:"日本語 ニュース""#,
    );
    let emoji = list_query_json(&paths.config_path, &paths.db_path, "tag:rust🦀");
    assert_eq!(cjk["total_count"], 1);
    assert_eq!(emoji["total_count"], 1);
}

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

#[test]
fn mark_read_uses_unread_tag_alias_when_unread_management_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_synced_fixture_with_unread_management_disabled(&temp);

    let all_items = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    let entry_ids = collect_item_ids(&all_items);
    assert_eq!(entry_ids.len(), 2);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("read")
        .arg(entry_ids[0].clone())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["updated_entry_count"], 0);
}

#[test]
fn mark_unread_uses_unread_tag_alias_when_unread_management_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_synced_fixture_with_unread_management_disabled(&temp);

    let all_items = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    let entry_ids = collect_item_ids(&all_items);
    assert_eq!(entry_ids.len(), 2);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("mark")
        .arg("unread")
        .arg(entry_ids[0].clone())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let data = extract_ok_data(&output);
    assert_eq!(data["updated_entry_count"], 1);

    let unread = list_query_json(&paths.config_path, &paths.db_path, "unread");
    assert_eq!(unread["total_count"], 1);
}
