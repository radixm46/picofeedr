//! CLI integration tests.

mod support;

#[path = "cli/config.rs"]
mod config;
#[path = "cli/feeds_status.rs"]
mod feeds_status;
#[path = "cli/list.rs"]
mod list;
#[path = "cli/output.rs"]
mod output;
#[path = "cli/sync.rs"]
mod sync;
#[path = "cli/view_mark.rs"]
mod view_mark;

use assert_cmd::cargo::cargo_bin_cmd;
use rusqlite::Connection;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::thread;
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

/// Extracts field value from plain status output.
fn status_plain_field<'a>(output: &'a str, key: &str) -> &'a str {
    let prefix = format!("{key}: ");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .expect("status plain field")
}

/// Returns true when a string looks like `YYYY-MM-DDTHH:MM:SS+09:00`.
fn looks_like_human_datetime(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 25 {
        return false;
    }
    bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'T'
        && bytes[11..13].iter().all(u8::is_ascii_digit)
        && bytes[13] == b':'
        && bytes[14..16].iter().all(u8::is_ascii_digit)
        && bytes[16] == b':'
        && bytes[17..19].iter().all(u8::is_ascii_digit)
        && (bytes[19] == b'+' || bytes[19] == b'-')
        && bytes[20..22].iter().all(u8::is_ascii_digit)
        && bytes[22] == b':'
        && bytes[23..25].iter().all(u8::is_ascii_digit)
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

/// Starts a local HTTP server that returns one 404 response.
fn spawn_http_404_feed_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind 404 server");
    let addr = listener.local_addr().expect("local addr");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept 404 request");
        let mut request_buf = [0_u8; 1024];
        let _ = stream.read(&mut request_buf);
        stream
            .write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
            )
            .expect("write 404 response");
        stream.flush().expect("flush 404 response");
    });
    (format!("http://{addr}/missing.xml"), handle)
}
