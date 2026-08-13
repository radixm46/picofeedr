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
    assert!(stdout.contains("(<expr>)"));
    assert!(stdout.contains("-(<expr>)"));
    assert!(stdout.contains("after:<YYYY-MM-DD|Nd|Nw|Nm|Ny>"));
    assert!(stdout.contains("before:<YYYY-MM-DD|Nd|Nw|Nm|Ny>"));
    assert!(!stdout.contains("title:\"<text>\""));
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

    let conn = Connection::open(&paths.db_path).expect("open database");
    let expected_entry_id: String = conn
        .query_row(
            "SELECT entry_id FROM entries WHERE title = ?1",
            ["Second Entry"],
            |row| row.get(0),
        )
        .expect("find second entry");
    let item = &data["items"][0];
    assert_eq!(item["entry_id"], expected_entry_id);
    assert_eq!(
        item["feed_id"],
        picofeedr::feed::feed_id_from_url(&format!(
            "file://{}",
            temp.path().join("feed.xml").display()
        ))
    );
    assert_eq!(item["title"], "Second Entry");
    assert_eq!(item["link"], "https://example.com/2");
    assert_eq!(item["published_at"], 1704153600);
    assert!(
        item["first_seen_at"]
            .as_i64()
            .is_some_and(|value| value > 0)
    );
    assert_eq!(item["tags"], serde_json::json!(["tech", "unread"]));

    let feeds = data["feeds"].as_array().expect("feeds array");
    assert_eq!(feeds.len(), 1);
    let feed = &feeds[0];
    assert_eq!(feed["feed_id"], item["feed_id"]);
    assert_eq!(feed["title"], "Example Feed");
}

#[test]
fn list_pagination_preserves_sort_order_for_simple_and_complex_paths() {
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

    let conn = Connection::open(&paths.db_path).expect("open database");
    let feed_pk: i64 = conn
        .query_row("SELECT id FROM feeds LIMIT 1", [], |row| row.get(0))
        .expect("find feed primary key");
    conn.execute(
        "UPDATE entries
         SET published_at = ?1, updated_at = ?2, first_seen_at = ?3
         WHERE title = ?4",
        rusqlite::params![200, 150, 300, "First Entry"],
    )
    .expect("update first entry timestamps");
    conn.execute(
        "UPDATE entries
         SET published_at = ?1, updated_at = ?2, first_seen_at = ?3
         WHERE title = ?4",
        rusqlite::params![Option::<i64>::None, 100, 100, "Second Entry"],
    )
    .expect("update second entry timestamps");
    conn.execute(
        "INSERT INTO entries
         (entry_id, feed_pk, title, link, published_at, updated_at, first_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            "entry-3",
            feed_pk,
            "Third Entry",
            "https://example.com/3",
            200,
            50,
            100
        ],
    )
    .expect("insert third entry");

    let titles = ["First Entry", "Second Entry", "Third Entry"];
    let mut entry_ids = Vec::with_capacity(titles.len());
    for title in titles {
        entry_ids.push(
            conn.query_row(
                "SELECT entry_id FROM entries WHERE title = ?1",
                [title],
                |row| row.get::<_, String>(0),
            )
            .expect("find entry id"),
        );
    }
    drop(conn);

    let list_page = |sort: &str, query: Option<&str>, cursor: Option<&str>| {
        let mut command = picofeedr_cmd_json();
        command
            .arg("--config")
            .arg(&paths.config_path)
            .arg("--storage-root")
            .arg(db_root(&paths.db_path))
            .arg("list")
            .arg("--sort")
            .arg(sort)
            .arg("--limit")
            .arg("1");
        if let Some(query) = query {
            command.arg("--query").arg(query);
        }
        if let Some(cursor) = cursor {
            command.arg("--cursor").arg(cursor);
        }
        let output = command.assert().success().get_output().stdout.clone();
        extract_result(&output, "ok")
    };

    let sort_cases = [
        ("date_desc", [2, 0, 1]),
        ("date_asc", [1, 0, 2]),
        ("first_seen_desc", [0, 2, 1]),
        ("first_seen_asc", [1, 2, 0]),
    ];
    for query in [None, Some("-tag:missing-pagination-tag")] {
        for (sort, expected_order) in sort_cases {
            let expected_ids = expected_order
                .into_iter()
                .map(|index| entry_ids[index].clone())
                .collect::<Vec<_>>();
            let mut actual_ids = Vec::new();
            let mut cursor = None;
            for page in 0..expected_ids.len() {
                let data = list_page(sort, query, cursor.as_deref());
                assert_eq!(data["total_count"], 3);
                assert_eq!(data["items"].as_array().expect("items array").len(), 1);
                actual_ids.extend(collect_item_ids(&data));
                cursor = data["next_page_token"].as_str().map(str::to_owned);
                if cursor.is_none() {
                    assert_eq!(page, expected_ids.len() - 1);
                    break;
                }
            }
            assert!(cursor.is_none(), "pagination should terminate");
            assert_eq!(actual_ids, expected_ids);
            assert_eq!(
                actual_ids
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                expected_ids.len()
            );
        }
    }
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
    for query in ["unread", "tag:tech|doesnotexist|m1|m2|m3|m4|m5"] {
        let data = list_query_json(&paths.config_path, &paths.db_path, query);
        assert_eq!(data["revision"], status["revision"]);
        assert_eq!(data["last_sync_at"], status["last_sync_at"]);
    }
}

#[test]
fn list_json_uses_unread_tag_alias_when_management_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_synced_fixture_with_unread_management_disabled(&temp);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query")
        .arg("unread tag:tech")
        .assert()
        .success()
        .get_output()
        .clone();

    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(!stderr.contains("warning: unread query ignored"));
    let data = extract_result(&output.stdout, "ok");
    assert_eq!(data["total_count"], 0);
}

#[test]
fn list_plain_uses_unread_tag_alias_when_management_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_synced_fixture_with_unread_management_disabled(&temp);

    let output = picofeedr_cmd_plain()
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
        .clone();

    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(!stderr.contains("warning: unread query ignored"));
    assert!(stderr.contains("total_count: 0"));
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
    let feed_id: String = Connection::open(&paths.db_path)
        .expect("open database")
        .query_row("SELECT feed_id FROM feeds LIMIT 1", [], |row| row.get(0))
        .expect("find feed id");
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
fn list_filters_by_bare_title_term() {
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
    let data = list_query_json(&paths.config_path, &paths.db_path, "First");
    assert_eq!(data["total_count"], 1);
}

#[test]
fn list_rejects_title_prefix_with_quote_hint() {
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
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let details = extract_error_details(&output);
    assert_eq!(details["kind"], "unknown_filter_prefix");
    assert_eq!(details["field"], "query");
    assert_eq!(details["value"], "title:\"First\"");
    assert_eq!(details["hint"], "quote_token_to_search_literal_text");
}

#[test]
fn list_title_term_treats_like_metacharacters_as_literals() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_title_literal_fixture(&temp);
    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    assert_single_title(
        list_query_json(&paths.config_path, &paths.db_path, "\"100%\""),
        "Budget 100% Launch",
    );
    assert_single_title(
        list_query_json(&paths.config_path, &paths.db_path, "\"A_B\""),
        "Build A_B Release",
    );
    assert_single_title(
        list_query_json(&paths.config_path, &paths.db_path, "\"C:\\\\Temp\""),
        "Path C:\\Temp Guide",
    );
}

#[test]
fn list_filters_by_title_terms_with_implicit_and_and_negation() {
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

    assert_single_title(
        list_query_json(&paths.config_path, &paths.db_path, "First"),
        "First Entry",
    );
    assert_single_title(
        list_query_json(&paths.config_path, &paths.db_path, "\"Second Entry\""),
        "Second Entry",
    );
    assert_single_title(
        list_query_json(&paths.config_path, &paths.db_path, "Entry -Second"),
        "First Entry",
    );
}

#[test]
fn list_accepts_hyphen_started_query_value_after_query_flag() {
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
        .arg("-Second")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("10")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_single_title(extract_result(&output, "ok"), "First Entry");
}

#[test]
fn list_filters_by_title_term_groups() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_term_group_fixture(&temp);
    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    assert_titles(
        list_query_json(
            &paths.config_path,
            &paths.db_path,
            "(alpha|アルファ) (beta|ベータ) -(gamma|ガンマ)",
        ),
        &["alpha beta Launch", "アルファ ベータ News"],
    );
    assert_titles(
        list_query_json(
            &paths.config_path,
            &paths.db_path,
            "((echo&delta)|\"共同声明\")",
        ),
        &["echo delta memo", "共同声明"],
    );
}

#[test]
fn list_rejects_operator_characters_inside_unquoted_bare_terms() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_term_group_fixture(&temp);
    picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("sync")
        .assert()
        .success();

    for raw in ["a|b", "a&b", "!foo", "-a|b", "-a&b", "-!foo"] {
        let output = picofeedr_cmd_json()
            .arg("--config")
            .arg(&paths.config_path)
            .arg("--storage-root")
            .arg(db_root(&paths.db_path))
            .arg("list")
            .arg("--query")
            .arg(raw)
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let details = extract_error_details(&output);
        assert_eq!(details["kind"], "bare_operator_token");
        assert_eq!(details["field"], "query");
        assert_eq!(details["value"], raw);
        assert_eq!(details["hint"], "quote_token_to_search_literal_text");
    }

    assert_single_title(
        list_query_json(&paths.config_path, &paths.db_path, "\"a|b\""),
        "Literal a|b",
    );
    assert_single_title(
        list_query_json(&paths.config_path, &paths.db_path, "Rust(2024)"),
        "Rust(2024)",
    );
}

#[test]
fn list_negated_title_group_matches_null_title_entries() {
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
    let conn = Connection::open(&paths.db_path).expect("open db");
    conn.execute(
        "UPDATE entries SET title = NULL WHERE title = 'First Entry'",
        [],
    )
    .expect("null title");

    let data = list_query_json(&paths.config_path, &paths.db_path, "-(First|Second)");
    assert_eq!(data["total_count"], 1);
    let items = data["items"].as_array().expect("items array");
    assert!(items[0]["title"].is_null());
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

fn assert_invalid_cursor(paths: &SyncFixturePaths, raw: &str, hint: &str, query: Option<&str>) {
    sync_fixture_ok(paths);
    let mut command = picofeedr_cmd_json();
    command
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list");
    if let Some(query) = query {
        command.arg("--query").arg(query);
    }
    let output = command
        .arg("--cursor")
        .arg(raw)
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = extract_error_payload(&output);
    assert_eq!(error["code"], "INVALID_QUERY");
    assert_eq!(
        error["details"],
        serde_json::json!({
            "kind": "invalid_cursor",
            "field": "cursor",
            "value": null,
            "hint": hint
        })
    );
    assert!(!String::from_utf8_lossy(&output).contains(raw));
}

#[test]
fn list_rejects_mismatched_cursor_without_echoing_raw_in_json_error() {
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
    let cursor = extract_result(&output, "ok")["next_page_token"]
        .as_str()
        .expect("cursor")
        .to_string();
    assert_invalid_cursor(&paths, &cursor, "cursor_mismatch", Some("tag:tech"));
}

#[test]
fn list_rejects_cursor_when_title_terms_change() {
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
        .arg("Entry")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cursor = extract_result(&output, "ok")["next_page_token"]
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
        .arg("First")
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
fn list_rejects_cursor_when_title_term_group_changes() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_term_group_fixture(&temp);
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
        .arg("--query=(alpha|アルファ)")
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cursor = extract_result(&output, "ok")["next_page_token"]
        .as_str()
        .expect("cursor")
        .to_string();
    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("list")
        .arg("--query=(beta|ベータ)")
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
fn list_rejects_invalid_cursor_format_without_echoing_raw_in_json_error() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    let raw = "not-a-cursor";
    assert_invalid_cursor(&paths, raw, "cursor_json_decode_failed", None);
}

#[test]
fn list_rejects_base64_invalid_cursor_without_echoing_raw_in_json_error() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    let raw = "!";
    assert_invalid_cursor(&paths, raw, "base64url_decode_failed", None);
}

#[test]
fn list_rejects_oversized_cursor_without_echoing_raw_in_json_error() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    let raw = "A".repeat(1025);
    assert_invalid_cursor(&paths, &raw, "cursor_too_long", None);
}

#[test]
fn list_uses_json_decode_error_at_cursor_byte_limit() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    let raw = "A".repeat(1024);
    assert_invalid_cursor(&paths, &raw, "cursor_json_decode_failed", None);
}

#[test]
fn list_rejects_cursor_over_byte_limit_even_when_character_count_is_lower() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_sync_fixture_files(&temp);
    let raw = "あ".repeat(513);
    assert!(raw.chars().count() <= 1024);
    assert!(raw.len() > 1024);
    assert_invalid_cursor(&paths, &raw, "cursor_too_long", None);
}

#[test]
fn list_uses_config_default_limit_when_limit_omitted() {
    let temp = TempDir::new().expect("tempdir");
    let paths = SyncFixtureBuilder::new(&temp).query_limits(1, 5).build_db();
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
        extract_result(&output, "ok")["items"]
            .as_array()
            .expect("items")
            .len(),
        1
    );
}

#[test]
fn list_rejects_limit_over_max_limit() {
    let temp = TempDir::new().expect("tempdir");
    let paths = SyncFixtureBuilder::new(&temp).query_limits(1, 5).build_db();
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
    let paths = SyncFixtureBuilder::new(&temp).query_limits(1, 5).build_db();
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

fn assert_single_title(data: serde_json::Value, expected: &str) {
    assert_eq!(data["total_count"], 1);
    let items = data["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], expected);
}

fn assert_titles(data: serde_json::Value, expected: &[&str]) {
    let actual = data["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| item["title"].as_str().expect("title").to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let expected = expected
        .iter()
        .map(|title| title.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(data["total_count"], expected.len() as i64);
}

fn write_title_literal_fixture(temp: &TempDir) -> SyncFixturePaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    let feed_path = temp.path().join("literal-feed.xml");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);

    let feed_url = format!("file://{}", feed_path.display());
    let feeds = format!(
        r#"picofeedr:
  tech:
    tags: [tech]
    feeds:
      - url: {feed_url}
        title: Literal Feed
"#
    );
    let feed = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Literal Feed</title>
    <link>https://example.com</link>
    <description>Literal Feed</description>
    <item>
      <title>Budget 100% Launch</title>
      <link>https://example.com/percent</link>
      <guid>literal-percent</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <description>Percent</description>
    </item>
    <item>
      <title>Budget 100X Launch</title>
      <link>https://example.com/percent-decoy</link>
      <guid>literal-percent-decoy</guid>
      <pubDate>Tue, 02 Jan 2024 00:00:00 GMT</pubDate>
      <description>Percent decoy</description>
    </item>
    <item>
      <title>Build A_B Release</title>
      <link>https://example.com/underscore</link>
      <guid>literal-underscore</guid>
      <pubDate>Wed, 03 Jan 2024 00:00:00 GMT</pubDate>
      <description>Underscore</description>
    </item>
    <item>
      <title>Build AXB Release</title>
      <link>https://example.com/underscore-decoy</link>
      <guid>literal-underscore-decoy</guid>
      <pubDate>Thu, 04 Jan 2024 00:00:00 GMT</pubDate>
      <description>Underscore decoy</description>
    </item>
    <item>
      <title>Path C:\Temp Guide</title>
      <link>https://example.com/backslash</link>
      <guid>literal-backslash</guid>
      <pubDate>Fri, 05 Jan 2024 00:00:00 GMT</pubDate>
      <description>Backslash</description>
    </item>
    <item>
      <title>Path C:Temp Guide</title>
      <link>https://example.com/backslash-decoy</link>
      <guid>literal-backslash-decoy</guid>
      <pubDate>Sat, 06 Jan 2024 00:00:00 GMT</pubDate>
      <description>Backslash decoy</description>
    </item>
  </channel>
</rss>
"#;

    fs::write(&feeds_path, feeds).expect("write feeds");
    fs::write(&feed_path, feed).expect("write feed");

    SyncFixturePaths {
        config_path: config_path.display().to_string(),
        db_path: db_path.display().to_string(),
    }
}

fn write_term_group_fixture(temp: &TempDir) -> SyncFixturePaths {
    let config_path = temp.path().join("config.toml");
    let feeds_path = temp.path().join("feeds.yaml");
    let db_path = temp.path().join("db.sqlite");
    let feed_path = temp.path().join("term-group-feed.xml");
    write_config_with_feeds_source(&config_path, &db_path, &feeds_path);

    let feed_url = format!("file://{}", feed_path.display());
    let feeds = format!(
        r#"picofeedr:
  tech:
    tags: [tech]
    feeds:
      - url: {feed_url}
        title: Term Group Feed
"#
    );
    let feed = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Term Group Feed</title>
    <link>https://example.com</link>
    <description>Term Group Feed</description>
    <item>
      <title>alpha beta Launch</title>
      <link>https://example.com/alpha-beta</link>
      <guid>group-alpha-beta</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <description>alpha beta</description>
    </item>
    <item>
      <title>アルファ ベータ News</title>
      <link>https://example.com/alpha-beta-ja</link>
      <guid>group-alpha-beta-ja</guid>
      <pubDate>Tue, 02 Jan 2024 00:00:00 GMT</pubDate>
      <description>Japanese alpha beta</description>
    </item>
    <item>
      <title>alpha gamma beta</title>
      <link>https://example.com/alpha-gamma-beta</link>
      <guid>group-alpha-gamma-beta</guid>
      <pubDate>Wed, 03 Jan 2024 00:00:00 GMT</pubDate>
      <description>gamma decoy</description>
    </item>
    <item>
      <title>echo delta memo</title>
      <link>https://example.com/echo-delta</link>
      <guid>group-echo-delta</guid>
      <pubDate>Thu, 04 Jan 2024 00:00:00 GMT</pubDate>
      <description>echo delta</description>
    </item>
    <item>
      <title>共同声明</title>
      <link>https://example.com/joint-statement</link>
      <guid>group-joint-statement</guid>
      <pubDate>Fri, 05 Jan 2024 00:00:00 GMT</pubDate>
      <description>Joint statement</description>
    </item>
    <item>
      <title>Literal a|b</title>
      <link>https://example.com/a-pipe-b</link>
      <guid>group-a-pipe-b</guid>
      <pubDate>Sat, 06 Jan 2024 00:00:00 GMT</pubDate>
      <description>Pipe literal</description>
    </item>
    <item>
      <title>Rust(2024)</title>
      <link>https://example.com/rust-2024</link>
      <guid>group-rust-2024</guid>
      <pubDate>Sun, 07 Jan 2024 00:00:00 GMT</pubDate>
      <description>Rust literal</description>
    </item>
  </channel>
</rss>
"#;

    fs::write(&feeds_path, feeds).expect("write feeds");
    fs::write(&feed_path, feed).expect("write feed");

    SyncFixturePaths {
        config_path: config_path.display().to_string(),
        db_path: db_path.display().to_string(),
    }
}
