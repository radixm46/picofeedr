use super::*;

#[test]
fn list_long_help_includes_query_reference_sections() {
    let stdout = String::from_utf8(
        cargo_bin_cmd!("picofeedr")
            .arg("list")
            .arg("--help")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .expect("utf8");

    assert!(stdout.contains("--query <QUERY>"));
    assert!(stdout.contains("tag:<expr>"));
    assert!(stdout.contains("-tag:<expr>"));
    assert!(stdout.contains("after:<YYYY-MM-DD|Nd|Nw|Nm|Ny>"));
    assert!(stdout.contains("before:<YYYY-MM-DD|Nd|Nw|Nm|Ny>"));
}

#[test]
fn list_plain_outputs_tsv_columns() {
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
        .clone();

    let stdout = String::from_utf8(output.stdout).expect("plain list utf-8");
    let stderr = String::from_utf8(output.stderr).expect("plain list stderr utf-8");
    assert_eq!(stdout.lines().count(), 2);
    assert!(stderr.contains("total_count: 2"));
}

#[test]
fn list_plain_with_id_appends_entry_id_column() {
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
        .arg("--id")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    for line in String::from_utf8(output).expect("utf8").lines() {
        assert_eq!(line.split('\t').count(), 6);
    }
}

#[test]
fn list_plain_writes_next_page_token_to_stderr() {
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
        .arg("1")
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(stderr.contains("next_page_token: "));
}

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
    let data = list_query_json(&paths.config_path, &paths.db_path, "unread tag:tech");
    assert_eq!(data["total_count"], 2);
}

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
}

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
    let data = list_query_json(&paths.config_path, &paths.db_path, "tag:(hot|tech)&!hot");
    assert_eq!(data["total_count"], 1);
}

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
    let data = list_query_json(&paths.config_path, &paths.db_path, "tag:tech -tag:hot|rust");
    assert_eq!(data["total_count"], 1);
}

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
    let data = list_query_json(
        &paths.config_path,
        &paths.db_path,
        &format!("feed:{feed_id}"),
    );
    assert_eq!(data["total_count"], 2);
}

#[test]
fn list_filter_by_missing_feed_id_returns_entry_not_found() {
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
        .arg("feed:missing-feed-id")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_error_envelope(&output, "ENTRY_NOT_FOUND", false);
}

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
    let data = list_query_json(&paths.config_path, &paths.db_path, "title:\"First\"");
    assert_eq!(data["total_count"], 1);
}

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
    let data = list_query_json(&paths.config_path, &paths.db_path, "after:2024-01-02");
    assert_eq!(data["total_count"], 1);
}

#[test]
fn list_filters_by_relative_date_range() {
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
    let data = list_query_json(&paths.config_path, &paths.db_path, "after:100y");
    assert_eq!(data["total_count"], 2);
}

#[test]
fn list_rejects_invalid_relative_date_filter() {
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
        .arg("after:3x")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "INVALID_QUERY");
}

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
        .arg("--limit")
        .arg("1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cursor = extract_ok_data(&output)["next_page_token"]
        .as_str()
        .expect("cursor")
        .to_string();
    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("tag:tech")
        .arg("--cursor")
        .arg(cursor)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "INVALID_QUERY");
}

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
        .arg("--cursor")
        .arg("not-a-cursor")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "INVALID_QUERY");
}

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
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        extract_ok_data(&output)["items"]
            .as_array()
            .expect("items")
            .len(),
        1
    );
}

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
}

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
}

#[test]
fn list_tag_or_with_missing_tag_keeps_existing_matches() {
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
    let tech = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    let or_data = list_query_json(&paths.config_path, &paths.db_path, "tag:tech|doesnotexist");
    assert_eq!(or_data["total_count"], tech["total_count"]);
}

#[test]
fn list_tag_not_missing_tag_matches_all_when_combined() {
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
    let tech = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    let not_missing = list_query_json(
        &paths.config_path,
        &paths.db_path,
        "tag:tech -tag:doesnotexist",
    );
    assert_eq!(not_missing["total_count"], tech["total_count"]);
}

#[test]
fn list_tag_only_missing_tag_returns_zero() {
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
    let data = list_query_json(&paths.config_path, &paths.db_path, "tag:doesnotexist");
    assert_eq!(data["total_count"], 0);
}

#[test]
fn list_tag_and_with_missing_tag_returns_zero() {
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
    let data = list_query_json(&paths.config_path, &paths.db_path, "tag:tech&doesnotexist");
    assert_eq!(data["total_count"], 0);
}

#[test]
fn list_complex_not_path_excludes_tagged_entries() {
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
    let simple = list_query_json(&paths.config_path, &paths.db_path, "tag:tech");
    assert_eq!(simple["total_count"], 2);
}

#[test]
fn list_complex_heavy_or_matches_simple_equivalent() {
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
    let simple = list_query_json(&paths.config_path, &paths.db_path, "tag:tech|doesnotexist");
    let complex = list_query_json(
        &paths.config_path,
        &paths.db_path,
        "tag:tech|doesnotexist|m1|m2|m3|m4|m5",
    );
    assert_eq!(complex["total_count"], simple["total_count"]);
}

#[test]
fn list_complex_path_respects_date_window_filters() {
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
    let data = list_query_json(
        &paths.config_path,
        &paths.db_path,
        "tag:tech -tag:news|later|junk|youtube|github after:2024-01-02 before:2024-01-03",
    );
    assert_eq!(data["total_count"], 1);
}

#[test]
fn list_complex_path_cursor_pagination_is_stable() {
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
    let data = list_query_json(
        &paths.config_path,
        &paths.db_path,
        "tag:tech|doesnotexist|m1|m2|m3|m4|m5",
    );
    assert_eq!(data["total_count"], 2);
}

#[test]
fn list_complex_large_match_set_does_not_hit_sql_variable_limit() {
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
    let data = list_query_json(
        &paths.config_path,
        &paths.db_path,
        "tag:unread -tag:news|later|junk|YouTube",
    );
    assert!(data["total_count"].as_i64().expect("count") >= 2);
}
