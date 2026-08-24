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
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;
use support::assertions::{
    assert_error_envelope, assert_plain_contract, extract_error_code, extract_error_details,
    extract_error_payload, extract_result,
};
use support::fixtures::{
    FixturePaths, SyncFixtureBuilder, SyncFixturePaths, acquire_exclusive_db_lock,
    write_fixture_files, write_sync_all_failed_fixture_files, write_sync_failure_fixture_files,
    write_sync_fixture_files, write_sync_fixture_files_fs,
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

fn fixture_cmd_json(
    config_path: impl AsRef<Path>,
    db_path: impl AsRef<Path>,
) -> assert_cmd::Command {
    let mut cmd = picofeedr_cmd_json();
    cmd.arg("--config")
        .arg(config_path.as_ref())
        .arg("--storage-root")
        .arg(db_root(db_path));
    cmd
}

fn fixture_cmd_plain(
    config_path: impl AsRef<Path>,
    db_path: impl AsRef<Path>,
) -> assert_cmd::Command {
    let mut cmd = picofeedr_cmd_plain();
    cmd.arg("--config")
        .arg(config_path.as_ref())
        .arg("--storage-root")
        .arg(db_root(db_path));
    cmd
}

/// Runs a successful sync for fixture paths.
fn sync_fixture_ok(paths: &SyncFixturePaths) {
    fixture_cmd_json(&paths.config_path, &paths.db_path)
        .arg("sync")
        .assert()
        .success();
}

/// Writes and syncs a fixture with unread management disabled.
fn write_synced_fixture_with_unread_management_disabled(temp: &TempDir) -> SyncFixturePaths {
    let paths = SyncFixtureBuilder::new(temp)
        .manage_unread(false)
        .build_db();
    sync_fixture_ok(&paths);
    paths
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
    let output = fixture_cmd_json(config_path, db_path)
        .arg("list")
        .arg(format!("--query={query}"))
        .arg("--sort")
        .arg("first_seen_desc")
        .arg("--limit")
        .arg("10")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    extract_result(&output, "ok")
}

/// Runs `status` in JSON mode and returns its `data` object.
fn status_json(config_path: &str, db_path: &str) -> serde_json::Value {
    let output = fixture_cmd_json(config_path, db_path)
        .arg("status")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    extract_result(&output, "ok")
}

/// Extracts field value from plain status output.
fn status_plain_field<'a>(output: &'a str, key: &str) -> &'a str {
    let prefix = format!("{key}: ");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .expect("status plain field")
}

/// Asserts that a human-readable timestamp is valid RFC3339.
fn assert_valid_rfc3339(value: &str) {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .expect("valid RFC3339 timestamp");
}

/// Resolves an entry id from the SQLite fixture.
fn entry_id_by_title(db_path: &str, title: &str) -> String {
    Connection::open(db_path)
        .expect("open database")
        .query_row(
            "SELECT entry_id FROM entries WHERE title = ?1",
            [title],
            |row| row.get(0),
        )
        .expect("entry id by title")
}

fn entry_ids_by_title(db_path: &str, titles: &[&str]) -> Vec<String> {
    titles
        .iter()
        .map(|title| entry_id_by_title(db_path, title))
        .collect()
}

/// Resolves root_dir from a db path.
fn db_root(db_path: impl AsRef<Path>) -> String {
    db_path
        .as_ref()
        .parent()
        .expect("db path should include a parent directory")
        .display()
        .to_string()
}

fn wait_for_server(done_rx: Receiver<()>, label: &str) {
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_else(|error| panic!("{label} did not complete: {error:?}"));
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
fn spawn_http_404_feed_server() -> (String, Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind 404 server");
    let addr = listener.local_addr().expect("local addr");
    let (done_tx, done_rx) = mpsc::channel();
    let _server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept 404 request");
        let mut request_buf = [0_u8; 1024];
        let _ = stream.read(&mut request_buf);
        stream
            .write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
            )
            .expect("write 404 response");
        stream.flush().expect("flush 404 response");
        done_tx.send(()).expect("404 server completion");
    });
    (format!("http://{addr}/missing.xml"), done_rx)
}
