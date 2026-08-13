//! Deterministic one-shot sync probe for external memory measurement.

use clap::Parser;
use picofeedr::config::{self, feeds::FeedsConfig};
use picofeedr::db::sqlite::SqliteStore;
use picofeedr::sync;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Parser)]
struct Args {
    /// Number of feeds to generate.
    #[arg(long)]
    feeds: usize,
    /// Number of entries per feed.
    #[arg(long)]
    entries: usize,
    /// Bytes to place in each entry description.
    #[arg(long, default_value_t = 256)]
    content_bytes: usize,
    /// Number of parallel sync workers.
    #[arg(long, default_value_t = 4)]
    parallel: usize,
}

struct ProbeDir {
    path: PathBuf,
}

impl ProbeDir {
    fn new() -> Result<Self, std::io::Error> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "picofeedr-sync-mem-probe-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ProbeDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn escape_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn render_config(root_dir: &Path, feeds_path: &Path, parallel: usize) -> String {
    format!(
        r#"unread_tag = "unread"

[feeds]
source = "{}"

[storage]
root_dir = "{}"

[sync]
parallel = {parallel}
timeout = 5
retry_count = 0
retry_delay = 0
user_agent = "picofeedr-mem-probe"
"#,
        escape_path(feeds_path),
        escape_path(root_dir),
    )
}

fn render_feed_xml(feed_idx: usize, entries_per_feed: usize, content_bytes: usize) -> String {
    let mut xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0">
  <channel>
    <title>Probe Feed {feed_idx}</title>
    <link>https://example.com/feed-{feed_idx}</link>
    <description>Probe feed {feed_idx}</description>
"#
    );
    let content = "x".repeat(content_bytes);
    for entry_idx in 0..entries_per_feed {
        let minute = entry_idx % 60;
        xml.push_str(&format!(
            r#"    <item>
      <title>Entry {feed_idx}-{entry_idx}</title>
      <link>https://example.com/feed-{feed_idx}/entry-{entry_idx}</link>
      <guid>feed-{feed_idx}-entry-{entry_idx}</guid>
      <pubDate>Mon, 01 Jan 2024 00:{minute:02}:00 GMT</pubDate>
      <description>{content}</description>
    </item>
"#
        ));
    }
    xml.push_str("  </channel>\n</rss>\n");
    xml
}

fn build_feeds_yaml(
    root: &Path,
    feed_count: usize,
    entries_per_feed: usize,
    content_bytes: usize,
) -> PathBuf {
    let feeds_path = root.join("feeds.yaml");
    let mut feeds_yaml = String::from("picofeedr:\n  probe:\n    tags: [probe]\n    feeds:\n");
    for feed_idx in 0..feed_count {
        let feed_path = root.join(format!("feed-{feed_idx}.xml"));
        fs::write(
            &feed_path,
            render_feed_xml(feed_idx, entries_per_feed, content_bytes),
        )
        .expect("write feed xml");
        feeds_yaml.push_str(&format!(
            "      - url: file://{}\n        title: Probe Feed {feed_idx}\n",
            feed_path.display()
        ));
    }
    fs::write(&feeds_path, feeds_yaml).expect("write feeds yaml");
    feeds_path
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let temp = ProbeDir::new()?;
    let feeds_path = build_feeds_yaml(temp.path(), args.feeds, args.entries, args.content_bytes);
    let config_path = temp.path().join("config.toml");
    fs::write(
        &config_path,
        render_config(temp.path(), &feeds_path, args.parallel),
    )?;

    let config = config::AppConfig::load(Some(config_path))?;
    let feeds_config = FeedsConfig::load(config.feeds_source())?;
    let mut store = SqliteStore::open(config.database_path())?;
    store.migrate()?;

    let summary = sync::run_sync(&mut store, &config, &feeds_config)?;
    println!(
        "status={} feeds={} failed={} new_entries={} duration_ms={}",
        summary.status.as_str(),
        summary.fetched_feed_count,
        summary.failed_feed_count,
        summary.new_entry_count,
        summary.duration_ms
    );
    Ok(())
}
