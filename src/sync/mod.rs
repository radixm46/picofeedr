//! Feed synchronization and ingestion.

mod autotag;
pub(crate) mod content;
mod fetch;
pub(crate) mod model;
mod normalize;

use crate::config::AppConfig;
use crate::config::feeds::FeedsConfig;
use crate::db::sqlite::SqliteStore;
use crate::error::AppError;
use crate::feed::feed_id_from_url;
use std::time::Instant;

pub use model::{SyncProgressEvent, SyncStatus, SyncSummary};

use autotag::compile_auto_tags;
use fetch::fetch_parallel;
use model::SyncTarget;

/// Runs a sync for all feeds in config.
pub fn run_sync(
    store: &mut SqliteStore,
    config: &AppConfig,
    feeds_config: &FeedsConfig,
) -> Result<SyncSummary, AppError> {
    run_sync_with_progress(store, config, feeds_config, None)
}

/// Runs a sync for all feeds in config and emits feed-level progress events.
pub fn run_sync_with_progress(
    store: &mut SqliteStore,
    config: &AppConfig,
    feeds_config: &FeedsConfig,
    mut on_progress: Option<&mut dyn FnMut(SyncProgressEvent)>,
) -> Result<SyncSummary, AppError> {
    let start = Instant::now();
    let targets = build_sync_targets(feeds_config)?;
    if let Some(progress) = on_progress.as_mut() {
        progress(SyncProgressEvent::Start {
            total_feeds: targets.len(),
        });
    }
    let (results, mut errors) = fetch_parallel(&targets, config, on_progress)?;
    let ingest = persist_sync_results(store, config, feeds_config, results)?;
    errors.extend(ingest.errors);

    let duration_ms = start.elapsed().as_millis() as u64;
    let failed_feed_count = errors.len();
    let status = derive_sync_status(targets.len(), failed_feed_count);
    Ok(SyncSummary {
        status,
        fetched_feed_count: targets.len(),
        failed_feed_count,
        new_entry_count: ingest.new_entry_count,
        duration_ms,
        errors,
    })
}

struct PersistOutcome {
    new_entry_count: usize,
    errors: Vec<model::SyncError>,
}

/// Persists feed metadata and ingests fetched results with per-feed transactions.
fn persist_sync_results(
    store: &mut SqliteStore,
    config: &AppConfig,
    feeds_config: &FeedsConfig,
    results: Vec<model::SyncResult>,
) -> Result<PersistOutcome, AppError> {
    let tx = store.tx()?;
    tx.feed_write_repo()
        .reconcile_feeds(feeds_config, &config.unread_tag)?;
    tx.commit()?;

    let feed_ids = results
        .iter()
        .map(|result| result.feed_id.clone())
        .collect::<Vec<_>>();
    let feed_pks_by_feed_id = store.feed_read_repo().find_feed_pks_by_ids(&feed_ids)?;
    let mut new_entry_count = 0;
    let mut errors = Vec::new();
    for result in results {
        let error_feed_id = result.feed_id.clone();
        let error_feed_name = result.feed_name.clone();
        let error_feed_url = result.feed_url.clone();
        let feed_pk = match feed_pks_by_feed_id.get(&result.feed_id).copied() {
            Some(feed_pk) => feed_pk,
            None => {
                errors.push(model::SyncError::ingest(
                    &error_feed_id,
                    error_feed_name.as_deref(),
                    &error_feed_url,
                    format!("Missing feed for {error_feed_id}"),
                ));
                continue;
            }
        };
        let tx = store.tx()?;
        match tx
            .sync_write_repo()
            .ingest_feed_result(config, feed_pk, result)
        {
            Ok(count) => {
                if let Err(error) = tx.commit() {
                    errors.push(model::SyncError::ingest(
                        &error_feed_id,
                        error_feed_name.as_deref(),
                        &error_feed_url,
                        error.to_string(),
                    ));
                    continue;
                }
                new_entry_count += count;
            }
            Err(error) => {
                errors.push(model::SyncError::ingest(
                    &error_feed_id,
                    error_feed_name.as_deref(),
                    &error_feed_url,
                    error.to_string(),
                ));
            }
        }
    }
    Ok(PersistOutcome {
        new_entry_count,
        errors,
    })
}

/// Computes sync status from failed feed count.
fn derive_sync_status(total_feeds: usize, failed_feed_count: usize) -> SyncStatus {
    if failed_feed_count > 0 && failed_feed_count == total_feeds {
        SyncStatus::Failed
    } else if failed_feed_count > 0 {
        SyncStatus::PartialFailed
    } else {
        SyncStatus::Completed
    }
}

/// Builds sync targets from feeds configuration.
fn build_sync_targets(feeds_config: &FeedsConfig) -> Result<Vec<SyncTarget>, AppError> {
    let mut targets = Vec::new();
    let total_feeds = feeds_config.feeds.len();
    for (offset, feed) in feeds_config.feeds.iter().enumerate() {
        let feed_id = feed_id_from_url(&feed.url);
        targets.push(SyncTarget {
            feed_id,
            feed_name: feed.title.clone(),
            url: feed.url.clone(),
            tags: feed.tags.clone(),
            auto_tag_rules: compile_auto_tags(&feed.auto_tags)?,
            index: offset + 1,
            total_feeds,
        });
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::model::{PendingEntry, SyncEntry, SyncResult};
    use super::{build_sync_targets, derive_sync_status, persist_sync_results};
    use crate::config::AppConfig;
    use crate::config::feeds::FeedsConfig;
    use crate::db::sqlite::SqliteStore;
    use tempfile::TempDir;

    fn escape_path(path: &std::path::Path) -> String {
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    }

    fn write_config_files(temp: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
        let feeds_path = temp.path().join("feeds.yaml");
        let config_path = temp.path().join("config.toml");
        let feeds_yaml = r#"
picofeedr:
  feeds:
    - url: "https://example.com/feed-a.xml"
      tags: ["tech"]
    - url: "https://example.com/feed-b.xml"
      tags: ["news"]
"#;
        std::fs::write(&feeds_path, feeds_yaml).expect("write feeds");

        let config_toml = format!(
            r#"
[feeds]
source = "{}"

[storage]
root_dir = "{}"

[sync]
parallel = 1
timeout = 1
user_agent = "picofeedr-test"
retry_count = 0
retry_delay = 0
"#,
            escape_path(&feeds_path),
            escape_path(temp.path()),
        );
        std::fs::write(&config_path, config_toml).expect("write config");
        (config_path, feeds_path)
    }

    fn make_result(feed_id: &str, entry_id: &str) -> SyncResult {
        SyncResult {
            feed_id: feed_id.to_string(),
            feed_name: Some(feed_id.to_string()),
            feed_url: format!("https://example.com/{feed_id}.xml"),
            entries: vec![SyncEntry {
                entry: PendingEntry {
                    entry_id: entry_id.to_string(),
                    link: Some(format!("https://example.com/{entry_id}")),
                    title: Some(entry_id.to_string()),
                    author: None,
                    published_at: None,
                    updated_at: None,
                    first_seen_at: 1,
                    meta_json: None,
                },
                content: None,
                content_payload: None,
                tags: vec!["tech".to_string()],
            }],
        }
    }

    #[test]
    fn derive_sync_status_maps_failed_count() {
        assert_eq!(derive_sync_status(2, 0), super::SyncStatus::Completed);
        assert_eq!(derive_sync_status(2, 1), super::SyncStatus::PartialFailed);
        assert_eq!(derive_sync_status(2, 2), super::SyncStatus::Failed);
    }

    #[test]
    fn persist_sync_results_counts_new_entries_across_feeds() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let results = vec![
            make_result(&targets[0].feed_id, "entry-a"),
            make_result(&targets[1].feed_id, "entry-b"),
        ];

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let count =
            persist_sync_results(&mut store, &config, &feeds_config, results).expect("persist");
        assert_eq!(count.new_entry_count, 2);
        assert!(count.errors.is_empty());

        let ids = vec!["entry-a".to_string(), "entry-b".to_string()];
        let found = store
            .entry_read_repo()
            .find_entry_pks_by_ids(&ids)
            .expect("find ids");
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn persist_sync_results_keeps_committed_feeds_on_later_error() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let results = vec![
            make_result(&targets[0].feed_id, "entry-a"),
            make_result("missing-feed-id", "entry-b"),
        ];

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let outcome = persist_sync_results(&mut store, &config, &feeds_config, results)
            .expect("persist with ingest errors");
        assert_eq!(outcome.new_entry_count, 1);
        assert_eq!(outcome.errors.len(), 1);
        assert_eq!(outcome.errors[0].feed_id, "missing-feed-id");
        assert_eq!(outcome.errors[0].code.as_str(), "INGEST_FAILED");
        assert!(outcome.errors[0].message.contains("Missing feed"));

        let ids = vec!["entry-a".to_string()];
        let found = store
            .entry_read_repo()
            .find_entry_pks_by_ids(&ids)
            .expect("find committed ids");
        assert_eq!(found.len(), 1);
    }
}
