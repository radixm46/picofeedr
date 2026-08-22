//! Feed synchronization and ingestion.

mod autotag;
pub(crate) mod content;
mod fetch;
mod gopher_transport;
pub(crate) mod model;
mod normalize;

use crate::config::AppConfig;
use crate::config::feeds::FeedsConfig;
use crate::db::sqlite::SqliteStore;
use crate::db::{EntryContentStorage, FeedInput, FeedMetadataInput};
use crate::error::AppError;
use crate::feed::feed_id_from_url;
use crate::sync::content::{remove_content_fs, write_content_fs};
use crate::time::current_epoch;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;
use tracing::warn;

pub use model::{SyncProgressEvent, SyncStatus, SyncSummary};

use autotag::compile_auto_tags;
use fetch::fetch_parallel;
use model::{FeedContext, SyncTarget};

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
    let skipped_feed_count = feeds_config.skipped_feeds().count();
    if let Some(progress) = on_progress.as_mut() {
        progress(SyncProgressEvent::Start {
            total_feeds: targets.len(),
            skipped_feed_count,
        });
        for feed in feeds_config.skipped_feeds() {
            progress(SyncProgressEvent::FeedSkip {
                url: feed.url.clone(),
                feed_name: feed.title.clone(),
            });
        }
    }
    let feed_pks_by_feed_id = prepare_sync_ingest(store, feeds_config, &targets)?;
    let mut new_entry_count = 0;
    let errors = fetch_parallel(&targets, config, on_progress, |result| {
        let count = ingest_sync_result(store, config, &feed_pks_by_feed_id, result)?;
        new_entry_count += count;
        Ok(())
    })?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let failed_feed_count = errors.len();
    let status = derive_sync_status(targets.len(), failed_feed_count);
    Ok(SyncSummary {
        status,
        fetched_feed_count: targets.len(),
        skipped_feed_count,
        failed_feed_count,
        new_entry_count,
        duration_ms,
        errors,
    })
}

/// Prepares feed metadata and resolves stable feed ids to primary keys.
fn prepare_sync_ingest(
    store: &mut SqliteStore,
    feeds_config: &FeedsConfig,
    targets: &[SyncTarget],
) -> Result<HashMap<String, i64>, AppError> {
    let feed_inputs = feeds_config
        .active_feeds()
        .map(feed_input)
        .collect::<Vec<_>>();
    let tx = store.tx()?;
    tx.ensure_feeds(&feed_inputs, current_epoch())?;
    tx.commit()?;

    let feed_ids = targets
        .iter()
        .map(|target| target.ctx.feed_id.clone())
        .collect::<Vec<_>>();
    store.find_feed_pks_by_ids(&feed_ids)
}

/// Builds database input for a configured active feed.
fn feed_input(feed: &crate::config::feeds::FeedConfig) -> FeedInput {
    FeedInput {
        feed_id: feed_id_from_url(&feed.url),
        url: feed.url.clone(),
        title: feed.title.clone(),
        author: None,
        site_url: None,
        meta_json: None,
    }
}

fn cleanup_created_content_files(
    data_dir: &Path,
    references: &[String],
    committed_refs: &HashSet<String>,
) {
    for reference in references {
        if !committed_refs.contains(reference)
            && let Err(error) = remove_content_fs(data_dir, reference)
        {
            warn!(
                data_dir = %data_dir.display(),
                reference = %reference,
                error = %error,
                "Failed to remove created content file during sync cleanup"
            );
        }
    }
}

fn cleanup_created_content_files_after_rollback(
    store: &mut SqliteStore,
    data_dir: &Path,
    references: &[String],
) {
    if references.is_empty() {
        return;
    }
    let tx = match store.immediate_tx() {
        Ok(tx) => tx,
        Err(error) => {
            warn!(
                data_dir = %data_dir.display(),
                error = %error,
                reference_count = references.len(),
                "Failed to begin created content cleanup transaction"
            );
            return;
        }
    };
    let committed_refs = match tx.entry_read_repo().find_content_refs(references) {
        Ok(committed_refs) => committed_refs,
        Err(error) => {
            warn!(
                data_dir = %data_dir.display(),
                error = %error,
                reference_count = references.len(),
                "Failed to find committed content references during sync cleanup"
            );
            return;
        }
    };
    cleanup_created_content_files(data_dir, references, &committed_refs);
    if let Err(error) = tx.commit() {
        warn!(
            data_dir = %data_dir.display(),
            error = %error,
            reference_count = references.len(),
            "Failed to commit created content cleanup transaction"
        );
    }
}

/// Ingests one fetched feed result and returns the number of newly inserted entries.
fn ingest_sync_result(
    store: &mut SqliteStore,
    config: &AppConfig,
    feed_pks_by_feed_id: &HashMap<String, i64>,
    result: model::SyncResult,
) -> Result<usize, model::SyncError> {
    let ctx = result.ctx.clone();
    let ingest_error = |message| model::SyncError::ingest(&ctx, message);
    let feed_pk = feed_pks_by_feed_id
        .get(&ctx.feed_id)
        .copied()
        .ok_or_else(|| ingest_error(format!("Missing feed for {}", ctx.feed_id)))?;
    let tx = store
        .tx()
        .map_err(|error| ingest_error(error.to_string()))?;
    let mut created_content_refs = Vec::new();
    let count = match (|| -> Result<usize, AppError> {
        let now = current_epoch();
        if result.feed_metadata.has_values() {
            tx.refresh_feed_metadata(
                feed_pk,
                &FeedMetadataInput {
                    title: result.feed_metadata.title.clone(),
                    author: result.feed_metadata.author.clone(),
                    site_url: result.feed_metadata.site_url.clone(),
                },
                now,
            )?;
        }
        let mut ingest = tx.ingest_context()?;
        let mut new_entries = 0;
        for entry in result.entries {
            let input = entry.entry.with_feed_pk(feed_pk);
            let insert = ingest.insert_entry(&input)?;
            if !insert.inserted {
                ingest.refresh_entry(insert.entry_pk, &input)?;
            }
            if let Some(content) = entry.content.as_ref() {
                if content.storage == EntryContentStorage::Fs {
                    let payload = entry.content_payload.as_deref().ok_or_else(|| {
                        AppError::internal("Missing content payload for fs storage")
                    })?;
                    let reference = content.reference.as_deref().ok_or_else(|| {
                        AppError::internal("Missing content reference for fs storage")
                    })?;
                    let created = write_content_fs(&config.storage.data_dir, reference, payload)?;
                    if created {
                        created_content_refs.push(reference.to_string());
                    }
                }
                ingest.insert_entry_content(insert.entry_pk, content)?;
            }
            ingest.replace_entry_enclosures(insert.entry_pk, &entry.enclosures)?;
            if insert.inserted {
                ingest.insert_entry_tags(insert.entry_pk, &entry.tags)?;
                new_entries += 1;
            }
        }
        Ok(new_entries)
    })() {
        Ok(count) => count,
        Err(error) => {
            // Cleanup is best effort so the primary ingest error remains the reported failure.
            drop(tx);
            cleanup_created_content_files_after_rollback(
                store,
                &config.storage.data_dir,
                &created_content_refs,
            );
            return Err(ingest_error(error.to_string()));
        }
    };
    if let Err(error) = tx.commit() {
        // Cleanup is best effort so the primary commit error remains the reported failure.
        cleanup_created_content_files_after_rollback(
            store,
            &config.storage.data_dir,
            &created_content_refs,
        );
        return Err(ingest_error(error.to_string()));
    }
    Ok(count)
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
    for feed in feeds_config.active_feeds() {
        let feed_id = feed_id_from_url(&feed.url);
        targets.push(SyncTarget {
            ctx: FeedContext {
                feed_id,
                feed_name: feed.title.clone(),
                url: feed.url.clone(),
            },
            tags: feed.tags.clone(),
            auto_tag_rules: compile_auto_tags(&feed.auto_tags)?,
        });
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::model::{
        FeedContext, FeedMetadata, PendingEntry, SyncEntry, SyncResult, SyncTarget,
    };
    use super::{
        build_sync_targets, cleanup_created_content_files,
        cleanup_created_content_files_after_rollback, derive_sync_status, ingest_sync_result,
        prepare_sync_ingest, run_sync,
    };
    use crate::config::AppConfig;
    use crate::config::feeds::FeedsConfig;
    use crate::content_ref::sha256_path;
    use crate::db::sqlite::SqliteStore;
    use crate::db::{EntryContentInput, EntryContentStorage, EntryEnclosureInput};
    use rusqlite::Connection;
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[derive(Clone)]
    struct LogWriter(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("log buffer lock")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

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

    fn write_single_feed_config_files(
        temp: &TempDir,
        feed_url: &str,
        title: Option<&str>,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let feeds_path = temp.path().join("feeds.yaml");
        let config_path = temp.path().join("config.toml");
        let title_line = title
            .map(|title| format!("      title: \"{title}\"\n"))
            .unwrap_or_default();
        let feeds_yaml = format!(
            "picofeedr:\n  feeds:\n    - url: \"{feed_url}\"\n{title_line}      tags: [tech]\n"
        );
        fs::write(&feeds_path, feeds_yaml).expect("write feeds");

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
        fs::write(&config_path, config_toml).expect("write config");
        (config_path, feeds_path)
    }

    fn write_atom_feed(
        path: &std::path::Path,
        remote_title: &str,
        author: Option<&str>,
        site_url: Option<&str>,
    ) {
        let author_xml = author
            .map(|name| format!("<author><name>{name}</name></author>"))
            .unwrap_or_default();
        let alternate_xml = site_url
            .map(|url| format!(r#"<link href="{url}" rel="alternate" />"#))
            .unwrap_or_default();
        let feed_xml = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>{remote_title}</title>
  <id>https://example.com/feed</id>
  <updated>2026-04-12T00:00:00Z</updated>
  {author_xml}
  {alternate_xml}
  <link href="https://example.com/feed.xml" rel="self" />
</feed>
"#
        );
        fs::write(path, feed_xml).expect("write atom feed");
    }

    fn make_result(feed_id: &str, entry_id: &str) -> SyncResult {
        SyncResult {
            ctx: FeedContext {
                feed_id: feed_id.to_string(),
                feed_name: Some(feed_id.to_string()),
                url: format!("https://example.com/{feed_id}.xml"),
            },
            feed_metadata: FeedMetadata::default(),
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
                enclosures: Vec::new(),
                tags: vec!["tech".to_string()],
            }],
        }
    }

    fn make_fs_result(feed_id: &str, entry_id: &str, reference: &str, payload: &str) -> SyncResult {
        let mut result = make_result(feed_id, entry_id);
        let entry = result.entries.first_mut().expect("result entry");
        entry.content = Some(EntryContentInput {
            storage: EntryContentStorage::Fs,
            reference: Some(reference.to_string()),
            content_type: Some("text/plain".to_string()),
            content: None,
        });
        entry.content_payload = Some(payload.to_string());
        result
    }

    fn setup_ingest(
        temp: &TempDir,
    ) -> (
        AppConfig,
        Vec<SyncTarget>,
        SqliteStore,
        HashMap<String, i64>,
    ) {
        let (config_path, feeds_path) = write_config_files(temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let feed_pks_by_feed_id =
            prepare_sync_ingest(&mut store, &feeds_config, &targets).expect("prepare");
        (config, targets, store, feed_pks_by_feed_id)
    }

    #[test]
    fn cleanup_failure_emits_warning_with_reference_and_error() {
        let temp = TempDir::new().expect("temp dir");
        let data_dir = temp.path().join("data");
        fs::create_dir_all(&data_dir).expect("data dir");
        let reference = "a".repeat(64);
        fs::write(data_dir.join("aa"), "blocked prefix").expect("blocked prefix");
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer_output = Arc::clone(&output);
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_ansi(false)
            .with_writer(move || LogWriter(Arc::clone(&writer_output)))
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            cleanup_created_content_files(
                &data_dir,
                std::slice::from_ref(&reference),
                &HashSet::new(),
            );
        });

        let output = String::from_utf8(output.lock().expect("log buffer lock").clone())
            .expect("utf8 log output");
        assert!(output.contains("Failed to remove created content file"));
        assert!(output.contains(&reference));
        assert!(output.contains("error=Failed to remove content:"));
    }

    #[test]
    fn cleanup_after_auto_rollback_preserves_committed_and_removes_unused_refs() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let committed_reference = "a".repeat(64);
        let unused_reference = "b".repeat(64);
        let committed_path =
            sha256_path(&config.storage.data_dir, &committed_reference).expect("content path");
        let unused_path =
            sha256_path(&config.storage.data_dir, &unused_reference).expect("content path");

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let feed_pks_by_feed_id =
            prepare_sync_ingest(&mut store, &feeds_config, &targets).expect("prepare");
        ingest_sync_result(
            &mut store,
            &config,
            &feed_pks_by_feed_id,
            make_fs_result(
                &targets[0].ctx.feed_id,
                "entry-committed-content",
                &committed_reference,
                "committed content",
            ),
        )
        .expect("commit content ingest");

        fs::create_dir_all(unused_path.parent().expect("content parent")).expect("content dir");
        fs::write(&unused_path, "unused content").expect("unused content file");
        let references = vec![committed_reference, unused_reference];

        cleanup_created_content_files_after_rollback(
            &mut store,
            &config.storage.data_dir,
            &references,
        );

        assert!(committed_path.exists());
        assert!(!unused_path.exists());
        store
            .bump_revision(2)
            .expect("subsequent write transaction after recovery");
    }

    #[test]
    fn derive_sync_status_maps_failed_count() {
        assert_eq!(derive_sync_status(2, 0), super::SyncStatus::Completed);
        assert_eq!(derive_sync_status(2, 1), super::SyncStatus::PartialFailed);
        assert_eq!(derive_sync_status(2, 2), super::SyncStatus::Failed);
    }

    #[test]
    fn prepare_and_ingest_sync_results_counts_new_entries_across_feeds() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let results = vec![
            make_result(&targets[0].ctx.feed_id, "entry-a"),
            make_result(&targets[1].ctx.feed_id, "entry-b"),
        ];

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let feed_pks_by_feed_id =
            prepare_sync_ingest(&mut store, &feeds_config, &targets).expect("prepare");
        let mut new_entry_count = 0;
        let mut errors = Vec::new();
        for result in results {
            match ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, result) {
                Ok(count) => new_entry_count += count,
                Err(error) => errors.push(error),
            }
        }
        assert_eq!(new_entry_count, 2);
        assert!(errors.is_empty());

        let ids = vec!["entry-a".to_string(), "entry-b".to_string()];
        let found = store
            .entry_read_repo()
            .find_entry_pks_by_ids(&ids)
            .expect("find ids");
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn ingest_tag_failure_preserves_preexisting_content_file() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let reference = "a".repeat(64);
        let content_path = sha256_path(&config.storage.data_dir, &reference).expect("content path");
        fs::create_dir_all(content_path.parent().expect("content parent")).expect("content dir");
        fs::write(&content_path, "existing").expect("existing content");

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let feed_pks_by_feed_id =
            prepare_sync_ingest(&mut store, &feeds_config, &targets).expect("prepare");
        let fail_conn = Connection::open(&config.database.path).expect("open trigger connection");
        fail_conn
            .execute_batch(
                "CREATE TRIGGER fail_tag_insert BEFORE INSERT ON tags
                 BEGIN SELECT RAISE(ABORT, 'forced tag failure'); END;",
            )
            .expect("install tag failure trigger");

        let error = ingest_sync_result(
            &mut store,
            &config,
            &feed_pks_by_feed_id,
            make_fs_result(&targets[0].ctx.feed_id, "entry-existing", &reference, "new"),
        )
        .expect_err("tag failure");
        drop(fail_conn);

        assert_eq!(error.code.as_str(), "INGEST_FAILED");
        assert!(error.message.contains("forced tag failure"));
        assert_eq!(
            fs::read_to_string(&content_path).expect("read content"),
            "existing"
        );
        assert!(
            store
                .entry_read_repo()
                .find_entry_pks_by_ids(&["entry-existing".to_string()])
                .expect("find rolled back entry")
                .is_empty()
        );
    }

    #[test]
    fn ingest_failure_preserves_recreated_content_referenced_before_rollback() {
        let temp = TempDir::new().expect("temp dir");
        let (config, targets, mut store, feed_pks_by_feed_id) = setup_ingest(&temp);
        let reference = "a".repeat(64);
        let content_path = sha256_path(&config.storage.data_dir, &reference).expect("content path");

        ingest_sync_result(
            &mut store,
            &config,
            &feed_pks_by_feed_id,
            make_fs_result(
                &targets[0].ctx.feed_id,
                "entry-referenced-content",
                &reference,
                "original",
            ),
        )
        .expect("initial ingest");
        fs::remove_file(&content_path).expect("remove content file");

        let fail_conn = Connection::open(&config.database.path).expect("open trigger connection");
        fail_conn
            .execute_batch(
                "CREATE TRIGGER fail_tag_insert BEFORE INSERT ON tags
                 BEGIN SELECT RAISE(ABORT, 'forced tag failure'); END;",
            )
            .expect("install tag failure trigger");

        let error = ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, {
            let mut result = make_fs_result(
                &targets[0].ctx.feed_id,
                "entry-referenced-content",
                &reference,
                "recreated",
            );
            result
                .entries
                .extend(make_result(&targets[0].ctx.feed_id, "entry-failing").entries);
            result
        })
        .expect_err("tag failure");
        drop(fail_conn);

        assert_eq!(error.code.as_str(), "INGEST_FAILED");
        assert!(error.message.contains("forced tag failure"));
        assert_eq!(
            fs::read_to_string(&content_path).expect("read recreated content"),
            "recreated"
        );
        let conn = Connection::open(&config.database.path).expect("open db");
        let persisted_reference: String = conn
            .query_row(
                "SELECT ec.ref FROM entry_contents ec
                 JOIN entries e ON e.id = ec.entry_pk
                 WHERE e.entry_id = 'entry-referenced-content'",
                [],
                |row| row.get(0),
            )
            .expect("read committed content reference");
        assert_eq!(persisted_reference, reference);
    }

    #[test]
    fn ingest_commit_failure_removes_new_content_file() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let first_reference = "b".repeat(64);
        let second_reference = "c".repeat(64);
        let first_content_path =
            sha256_path(&config.storage.data_dir, &first_reference).expect("content path");
        let second_content_path =
            sha256_path(&config.storage.data_dir, &second_reference).expect("content path");

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let feed_pks_by_feed_id =
            prepare_sync_ingest(&mut store, &feeds_config, &targets).expect("prepare");
        let fail_conn = Connection::open(&config.database.path).expect("open trigger connection");
        fail_conn
            .execute_batch(
                "CREATE TABLE commit_parent (id INTEGER PRIMARY KEY);
                 CREATE TABLE commit_child (
                     parent_id INTEGER NOT NULL,
                     FOREIGN KEY(parent_id) REFERENCES commit_parent(id)
                         DEFERRABLE INITIALLY DEFERRED
                 );
                 CREATE TRIGGER fail_commit AFTER INSERT ON entries
                 BEGIN
                     INSERT INTO commit_child(parent_id) VALUES (-1);
                 END;",
            )
            .expect("install commit failure trigger");

        let error = ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, {
            let mut result = make_fs_result(
                &targets[0].ctx.feed_id,
                "entry-commit-failure",
                &first_reference,
                "first content",
            );
            result.entries.extend(
                make_fs_result(
                    &targets[0].ctx.feed_id,
                    "entry-commit-failure-2",
                    &second_reference,
                    "second content",
                )
                .entries,
            );
            result
        })
        .expect_err("commit failure");
        drop(fail_conn);

        assert_eq!(error.code.as_str(), "INGEST_FAILED");
        assert!(error.message.contains("FOREIGN KEY constraint failed"));
        assert!(!first_content_path.exists());
        assert!(!second_content_path.exists());
        assert!(
            store
                .entry_read_repo()
                .find_entry_pks_by_ids(&[
                    "entry-commit-failure".to_string(),
                    "entry-commit-failure-2".to_string(),
                ])
                .expect("find rolled back entry")
                .is_empty()
        );
    }

    #[test]
    fn ingest_success_keeps_new_content_file() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let reference = "d".repeat(64);
        let content_path = sha256_path(&config.storage.data_dir, &reference).expect("content path");

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let feed_pks_by_feed_id =
            prepare_sync_ingest(&mut store, &feeds_config, &targets).expect("prepare");

        let count = ingest_sync_result(
            &mut store,
            &config,
            &feed_pks_by_feed_id,
            make_fs_result(&targets[0].ctx.feed_id, "entry-success", &reference, "new"),
        )
        .expect("successful ingest");

        assert_eq!(count, 1);
        assert_eq!(
            fs::read_to_string(&content_path).expect("read content"),
            "new"
        );
    }

    #[test]
    fn ingest_existing_entry_replaces_content_and_leaves_old_fs_file_for_maintenance() {
        let temp = TempDir::new().expect("tempdir");
        let (config, targets, mut store, feed_pks_by_feed_id) = setup_ingest(&temp);
        let old_reference = "e".repeat(64);
        let old_path = sha256_path(&config.storage.data_dir, &old_reference).expect("old path");
        ingest_sync_result(
            &mut store,
            &config,
            &feed_pks_by_feed_id,
            make_fs_result(
                &targets[0].ctx.feed_id,
                "entry-refresh",
                &old_reference,
                "old",
            ),
        )
        .expect("initial ingest");

        let mut refreshed = make_result(&targets[0].ctx.feed_id, "entry-refresh");
        refreshed.entries[0].content = Some(EntryContentInput {
            storage: EntryContentStorage::Db,
            reference: None,
            content_type: Some("text/plain".to_string()),
            content: Some("new".to_string()),
        });
        ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, refreshed)
            .expect("refresh ingest");

        assert!(old_path.exists());
        let conn = Connection::open(&config.database.path).expect("open db");
        let row: (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT storage, ref, content FROM entry_contents WHERE entry_pk = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("content row");
        assert_eq!(row, ("db".to_string(), None, Some("new".to_string())));
        ingest_sync_result(
            &mut store,
            &config,
            &feed_pks_by_feed_id,
            make_result(&targets[0].ctx.feed_id, "entry-refresh"),
        )
        .expect("absent content ingest");

        let content: Option<String> = conn
            .query_row(
                "SELECT content FROM entry_contents WHERE entry_pk = 1",
                [],
                |row| row.get(0),
            )
            .expect("content row");
        assert_eq!(content.as_deref(), Some("new"));
    }

    #[test]
    fn ingest_replaces_non_empty_enclosures_and_clears_empty_observations() {
        let temp = TempDir::new().expect("tempdir");
        let (config, targets, mut store, feed_pks_by_feed_id) = setup_ingest(&temp);

        let mut initial = make_result(&targets[0].ctx.feed_id, "entry-enclosures");
        initial.entries[0].enclosures = vec![EntryEnclosureInput {
            url: "https://example.com/old.mp3".to_string(),
            mime_type: Some("audio/mpeg".to_string()),
            length: Some(1),
        }];
        ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, initial)
            .expect("initial ingest");

        let mut refreshed = make_result(&targets[0].ctx.feed_id, "entry-enclosures");
        refreshed.entries[0].enclosures = vec![EntryEnclosureInput {
            url: "https://example.com/new.mp3".to_string(),
            mime_type: Some("audio/mpeg".to_string()),
            length: Some(2),
        }];
        ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, refreshed)
            .expect("refresh ingest");

        ingest_sync_result(
            &mut store,
            &config,
            &feed_pks_by_feed_id,
            make_result(&targets[0].ctx.feed_id, "entry-enclosures"),
        )
        .expect("empty observation ingest");

        let conn = Connection::open(&config.database.path).expect("open db");
        let rows: Vec<(String, Option<String>, Option<i64>)> = {
            let mut stmt = conn
                .prepare("SELECT url, mime_type, length FROM entry_enclosures ORDER BY id")
                .expect("prepare enclosures");
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .expect("query enclosures")
                .collect::<Result<_, _>>()
                .expect("collect enclosures")
        };
        assert!(rows.is_empty());
    }

    #[test]
    fn ingest_refreshes_source_metadata_clears_missing_dates_and_preserves_first_seen_and_tags() {
        let temp = TempDir::new().expect("tempdir");
        let (config, targets, mut store, feed_pks_by_feed_id) = setup_ingest(&temp);

        let mut initial = make_result(&targets[0].ctx.feed_id, "entry-refresh-metadata");
        initial.entries[0].entry.author = Some("Original author".to_string());
        initial.entries[0].entry.published_at = Some(10);
        initial.entries[0].entry.updated_at = Some(11);
        initial.entries[0].entry.first_seen_at = 12;
        initial.entries[0].entry.meta_json = Some("{\"old\":true}".to_string());
        assert_eq!(
            ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, initial,)
                .expect("initial ingest"),
            1
        );

        let mut refreshed = make_result(&targets[0].ctx.feed_id, "entry-refresh-metadata");
        refreshed.entries[0].entry.link = Some("https://example.com/refreshed".to_string());
        refreshed.entries[0].entry.title = Some("Refreshed title".to_string());
        refreshed.entries[0].entry.author = None;
        refreshed.entries[0].entry.published_at = None;
        refreshed.entries[0].entry.updated_at = None;
        refreshed.entries[0].entry.first_seen_at = 99;
        refreshed.entries[0].entry.meta_json = Some("{\"new\":true}".to_string());
        assert_eq!(
            ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, refreshed,)
                .expect("refresh ingest"),
            0
        );

        let conn = Connection::open(&config.database.path).expect("open db");
        let row: (
            String,
            String,
            Option<String>,
            Option<i64>,
            Option<i64>,
            i64,
            String,
        ) = conn
            .query_row(
                "SELECT link, title, author, published_at, updated_at, first_seen_at, meta_json
                 FROM entries WHERE entry_id = 'entry-refresh-metadata'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .expect("read entry");
        assert_eq!(
            row,
            (
                "https://example.com/refreshed".to_string(),
                "Refreshed title".to_string(),
                Some("Original author".to_string()),
                None,
                None,
                12,
                "{\"new\":true}".to_string()
            )
        );
        let effective_date: i64 = conn
            .query_row(
                "SELECT COALESCE(published_at, updated_at, first_seen_at)
                 FROM entries WHERE entry_id = 'entry-refresh-metadata'",
                [],
                |row| row.get(0),
            )
            .expect("read effective date");
        assert_eq!(effective_date, 12);
        let tag_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entry_tags WHERE entry_pk = (SELECT id FROM entries WHERE entry_id = 'entry-refresh-metadata')",
                [],
                |row| row.get(0),
            )
            .expect("count tags");
        assert_eq!(tag_count, 1);
        let content_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM entry_contents", [], |row| row.get(0))
            .expect("count content rows");
        assert_eq!(content_rows, 0);
    }

    #[test]
    fn ingest_refresh_clears_unobserved_entry_metadata() {
        let temp = TempDir::new().expect("tempdir");
        let (config, targets, mut store, feed_pks_by_feed_id) = setup_ingest(&temp);

        let mut initial = make_result(&targets[0].ctx.feed_id, "entry-clears-metadata");
        initial.entries[0].entry.meta_json = Some("{\"old\":true}".to_string());
        ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, initial)
            .expect("initial ingest");

        ingest_sync_result(
            &mut store,
            &config,
            &feed_pks_by_feed_id,
            make_result(&targets[0].ctx.feed_id, "entry-clears-metadata"),
        )
        .expect("refresh ingest");

        let conn = Connection::open(&config.database.path).expect("open db");
        let meta_json: Option<String> = conn
            .query_row(
                "SELECT meta_json FROM entries WHERE entry_id = 'entry-clears-metadata'",
                [],
                |row| row.get(0),
            )
            .expect("read entry metadata");
        assert_eq!(meta_json, None);
    }

    #[test]
    fn ingest_tag_failure_removes_new_content_file() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let reference = "c".repeat(64);
        let content_path = sha256_path(&config.storage.data_dir, &reference).expect("content path");

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let feed_pks_by_feed_id =
            prepare_sync_ingest(&mut store, &feeds_config, &targets).expect("prepare");
        let fail_conn = Connection::open(&config.database.path).expect("open trigger connection");
        fail_conn
            .execute_batch(
                "CREATE TRIGGER fail_tag_insert BEFORE INSERT ON tags
                 BEGIN SELECT RAISE(ABORT, 'forced tag failure'); END;",
            )
            .expect("install tag failure trigger");

        let error = ingest_sync_result(
            &mut store,
            &config,
            &feed_pks_by_feed_id,
            make_fs_result(
                &targets[0].ctx.feed_id,
                "entry-tag-failure",
                &reference,
                "new",
            ),
        )
        .expect_err("tag failure");
        drop(fail_conn);

        assert_eq!(error.code.as_str(), "INGEST_FAILED");
        assert!(error.message.contains("forced tag failure"));
        assert!(!content_path.exists());
        assert!(
            store
                .entry_read_repo()
                .find_entry_pks_by_ids(&["entry-tag-failure".to_string()])
                .expect("find rolled back entry")
                .is_empty()
        );
    }

    #[test]
    fn sequential_ingest_keeps_committed_feeds_on_later_error() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");
        let results = vec![
            make_result(&targets[0].ctx.feed_id, "entry-a"),
            make_result("missing-feed-id", "entry-b"),
        ];

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let feed_pks_by_feed_id =
            prepare_sync_ingest(&mut store, &feeds_config, &targets).expect("prepare");
        let mut new_entry_count = 0;
        let mut errors = Vec::new();
        for result in results {
            match ingest_sync_result(&mut store, &config, &feed_pks_by_feed_id, result) {
                Ok(count) => new_entry_count += count,
                Err(error) => errors.push(error),
            }
        }
        assert_eq!(new_entry_count, 1);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].feed_id, "missing-feed-id");
        assert_eq!(errors[0].code.as_str(), "INGEST_FAILED");
        assert!(errors[0].message.contains("Missing feed"));

        let ids = vec!["entry-a".to_string()];
        let found = store
            .entry_read_repo()
            .find_entry_pks_by_ids(&ids)
            .expect("find committed ids");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn prepare_sync_ingest_resolves_feed_pks_for_all_targets() {
        let temp = TempDir::new().expect("temp dir");
        let (config_path, feeds_path) = write_config_files(&temp);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let targets = build_sync_targets(&feeds_config).expect("build targets");

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let feed_pks_by_feed_id =
            prepare_sync_ingest(&mut store, &feeds_config, &targets).expect("prepare");

        assert_eq!(feed_pks_by_feed_id.len(), targets.len());
        for target in &targets {
            assert!(feed_pks_by_feed_id.contains_key(&target.ctx.feed_id));
        }
    }

    #[test]
    fn run_sync_populates_feed_metadata_cache_without_overwriting_config_title() {
        let temp = TempDir::new().expect("temp dir");
        let feed_path = temp.path().join("feed.xml");
        write_atom_feed(
            &feed_path,
            "Remote Title",
            Some("Remote Author"),
            Some("https://example.com/site"),
        );
        let feed_url = format!("file://{}", feed_path.to_string_lossy());
        let (config_path, feeds_path) =
            write_single_feed_config_files(&temp, &feed_url, Some("Configured Title"));
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        let summary = run_sync(&mut store, &config, &feeds_config).expect("run sync");
        assert_eq!(summary.new_entry_count, 0);

        let feeds = store.list_feeds().expect("list feeds");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].title.as_deref(), Some("Configured Title"));
        assert_eq!(feeds[0].author.as_deref(), Some("Remote Author"));
        assert_eq!(
            feeds[0].site_url.as_deref(),
            Some("https://example.com/site")
        );
    }

    #[test]
    fn run_sync_keeps_existing_feed_metadata_when_latest_fetch_has_empty_values() {
        let temp = TempDir::new().expect("temp dir");
        let feed_path = temp.path().join("feed.xml");
        let feed_url = format!("file://{}", feed_path.to_string_lossy());
        let (config_path, feeds_path) =
            write_single_feed_config_files(&temp, &feed_url, Some("Configured Title"));
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");

        write_atom_feed(
            &feed_path,
            "Remote Title",
            Some("Remote Author"),
            Some("https://example.com/site"),
        );
        run_sync(&mut store, &config, &feeds_config).expect("initial sync");

        write_atom_feed(&feed_path, "Remote Title", None, None);
        run_sync(&mut store, &config, &feeds_config).expect("second sync");

        let feeds = store.list_feeds().expect("list feeds");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].author.as_deref(), Some("Remote Author"));
        assert_eq!(
            feeds[0].site_url.as_deref(),
            Some("https://example.com/site")
        );
        assert_eq!(feeds[0].title.as_deref(), Some("Configured Title"));
    }

    #[test]
    fn run_sync_fills_missing_config_title_from_observed_feed_title() {
        let temp = TempDir::new().expect("temp dir");
        let feed_path = temp.path().join("feed.xml");
        write_atom_feed(
            &feed_path,
            "Remote Title",
            Some("Remote Author"),
            Some("https://example.com/site"),
        );
        let feed_url = format!("file://{}", feed_path.to_string_lossy());
        let (config_path, feeds_path) = write_single_feed_config_files(&temp, &feed_url, None);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");

        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");
        run_sync(&mut store, &config, &feeds_config).expect("run sync");

        let feeds = store.list_feeds().expect("list feeds");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].title.as_deref(), Some("Remote Title"));
    }

    #[test]
    fn run_sync_keeps_existing_fallback_title_when_latest_fetch_has_empty_title() {
        let temp = TempDir::new().expect("temp dir");
        let feed_path = temp.path().join("feed.xml");
        let feed_url = format!("file://{}", feed_path.to_string_lossy());
        let (config_path, feeds_path) = write_single_feed_config_files(&temp, &feed_url, None);
        let config = AppConfig::load(Some(config_path)).expect("load config");
        let feeds_config = FeedsConfig::load(&feeds_path).expect("load feeds");
        let mut store = SqliteStore::open(&config.database.path).expect("open store");
        store.migrate().expect("migrate");

        write_atom_feed(
            &feed_path,
            "Remote Title",
            Some("Remote Author"),
            Some("https://example.com/site"),
        );
        run_sync(&mut store, &config, &feeds_config).expect("initial sync");

        write_atom_feed(
            &feed_path,
            "",
            Some("Remote Author"),
            Some("https://example.com/site"),
        );
        run_sync(&mut store, &config, &feeds_config).expect("second sync");

        let feeds = store.list_feeds().expect("list feeds");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].title.as_deref(), Some("Remote Title"));
    }
}
