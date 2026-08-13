use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use picofeedr::config::{self, feeds::FeedsConfig};
use picofeedr::db::sqlite::SqliteStore;
use picofeedr::sync;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct SyncCorpus {
    _temp: TempDir,
    feeds_path: PathBuf,
    feeds_config: FeedsConfig,
    expected_feed_count: usize,
    expected_new_entries: usize,
}

struct SyncRunFixture {
    _temp: TempDir,
    store: SqliteStore,
    config: config::AppConfig,
    feeds_config: FeedsConfig,
    expected_feed_count: usize,
    expected_new_entries: usize,
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
user_agent = "picofeedr-bench"
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
    <title>Bench Feed {feed_idx}</title>
    <link>https://example.com/feed-{feed_idx}</link>
    <description>Bench feed {feed_idx}</description>
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

fn build_corpus(feed_count: usize, entries_per_feed: usize, content_bytes: usize) -> SyncCorpus {
    let temp = TempDir::new().expect("tempdir");
    let feeds_path = temp.path().join("feeds.yaml");
    let mut feeds_yaml = String::from("picofeedr:\n  bench:\n    tags: [bench]\n    feeds:\n");

    for feed_idx in 0..feed_count {
        let feed_path = temp.path().join(format!("feed-{feed_idx}.xml"));
        fs::write(
            &feed_path,
            render_feed_xml(feed_idx, entries_per_feed, content_bytes),
        )
        .expect("write feed xml");
        feeds_yaml.push_str(&format!(
            "      - url: file://{}\n        title: Bench Feed {feed_idx}\n",
            feed_path.display()
        ));
    }

    fs::write(&feeds_path, feeds_yaml).expect("write feeds yaml");
    let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds config");
    SyncCorpus {
        _temp: temp,
        feeds_path,
        feeds_config,
        expected_feed_count: feed_count,
        expected_new_entries: feed_count * entries_per_feed,
    }
}

fn build_run_fixture(corpus: &SyncCorpus, parallel: usize) -> SyncRunFixture {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config.toml");
    fs::write(
        &config_path,
        render_config(temp.path(), &corpus.feeds_path, parallel),
    )
    .expect("write config");

    let config = config::AppConfig::load(Some(config_path)).expect("load config");
    let store = SqliteStore::open(config.database_path()).expect("open sqlite");
    store.migrate().expect("migrate schema");

    SyncRunFixture {
        _temp: temp,
        store,
        config,
        feeds_config: corpus.feeds_config.clone(),
        expected_feed_count: corpus.expected_feed_count,
        expected_new_entries: corpus.expected_new_entries,
    }
}

fn bench_sync_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync_throughput");
    group.sample_size(10);

    let cases = [
        ("10x100", 10usize, 100usize),
        ("50x100", 50usize, 100usize),
        ("100x100", 100usize, 100usize),
    ];

    for &(label, feed_count, entries_per_feed) in &cases {
        let corpus = build_corpus(feed_count, entries_per_feed, 256);
        group.throughput(Throughput::Elements(corpus.expected_new_entries as u64));
        group.bench_with_input(BenchmarkId::new("case", label), &label, |b, _| {
            b.iter_batched(
                || build_run_fixture(&corpus, 4),
                |mut fixture| {
                    let summary =
                        sync::run_sync(&mut fixture.store, &fixture.config, &fixture.feeds_config)
                            .expect("sync result");
                    assert_eq!(summary.status.as_str(), "completed");
                    assert_eq!(summary.fetched_feed_count, fixture.expected_feed_count);
                    assert_eq!(summary.failed_feed_count, 0);
                    assert!(summary.errors.is_empty());
                    assert_eq!(summary.new_entry_count, fixture.expected_new_entries);
                    criterion::black_box(summary.new_entry_count);
                },
                BatchSize::PerIteration,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, bench_sync_throughput);
criterion_main!(benches);
