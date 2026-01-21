//! Feed synchronization and ingestion.

use crate::config::feeds::{AutoTagRule, FeedsConfig};
use crate::config::{AppConfig, ContentStore};
use crate::db::sqlite::SqliteStore;
use crate::db::{EntryContentInput, EntryInput};
use crate::error::AppError;
use crate::feed::feed_key_from_url;
use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// Sync result summary.
#[derive(Debug, Serialize)]
pub struct SyncSummary {
    /// Sync status string.
    pub status: String,
    /// Number of feeds fetched.
    pub fetched: usize,
    /// Number of failed feeds.
    pub failed: usize,
    /// Number of new entries ingested.
    pub new_entries: usize,
    /// Elapsed time in seconds.
    pub elapsed: f64,
    /// Sync errors for failed feeds.
    pub errors: Vec<SyncError>,
}

/// Runs a sync for all feeds in config.
pub fn run_sync(
    store: &SqliteStore,
    config: &AppConfig,
    feeds_config: &FeedsConfig,
) -> Result<SyncSummary, AppError> {
    let start = Instant::now();
    crate::feed::reconcile_feeds(store, feeds_config, &config.unread_tag)?;

    let compiled_rules = Arc::new(compile_auto_tags(&feeds_config.auto_tags)?);
    let targets = build_sync_targets(store, feeds_config)?;
    let (results, errors) = fetch_parallel(&targets, config, Arc::clone(&compiled_rules))?;

    let mut new_entries = 0;
    for result in results {
        for entry in result.entries {
            let insert = store.insert_entry(&entry.entry)?;
            if insert.inserted {
                if let Some(content) = entry.content {
                    store.insert_entry_content(insert.entry_id, &content)?;
                }
                store.insert_entry_tags(insert.entry_id, &entry.tags)?;
                new_entries += 1;
            }
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let failed = errors.len();
    let status = if failed > 0 && failed == targets.len() {
        "failed".to_string()
    } else if failed > 0 {
        "partial_failed".to_string()
    } else {
        "completed".to_string()
    };
    Ok(SyncSummary {
        status,
        fetched: targets.len(),
        failed,
        new_entries,
        elapsed,
        errors,
    })
}

/// Sync target with feed metadata and tags.
#[derive(Debug, Clone)]
struct SyncTarget {
    feed_id: i64,
    feed_key: String,
    url: String,
    tags: Vec<String>,
}

/// Parsed feed result from fetch workers.
#[derive(Debug)]
struct SyncResult {
    entries: Vec<SyncEntry>,
}

/// Normalized entry with tags and content payload.
#[derive(Debug)]
struct SyncEntry {
    entry: EntryInput,
    content: Option<EntryContentInput>,
    tags: Vec<String>,
}

/// Worker result returned from fetch threads.
#[derive(Debug)]
enum WorkerResult {
    /// Parsed feed result.
    Ok(SyncResult),
    /// Non-fatal sync error for a feed.
    Error(SyncError),
    /// Fatal error that should abort sync.
    Fatal(AppError),
}

/// Sync error entry for failed feeds.
#[derive(Debug, Serialize, Clone)]
pub struct SyncError {
    /// Feed URL that failed.
    pub feed_url: String,
    /// Error code string.
    pub code: String,
    /// Error message.
    pub message: String,
    /// Whether the caller should retry.
    pub retry: bool,
}

/// Auto-tag rule compiled for matching.
#[derive(Debug, Clone)]
struct CompiledRule {
    regex: Option<Regex>,
    contains: Vec<String>,
    add_tags: Vec<String>,
    priority: i64,
}

/// Builds sync targets from feeds configuration.
fn build_sync_targets(
    store: &SqliteStore,
    feeds_config: &FeedsConfig,
) -> Result<Vec<SyncTarget>, AppError> {
    let mut targets = Vec::new();
    for feed in &feeds_config.feeds {
        let feed_key = feed_key_from_url(&feed.url);
        let feed_id = store
            .find_feed_id(&feed_key)?
            .ok_or_else(|| AppError::db(format!("Feed missing for key {feed_key}")))?;
        targets.push(SyncTarget {
            feed_id,
            feed_key,
            url: feed.url.clone(),
            tags: feed.tags.clone(),
        });
    }
    Ok(targets)
}

/// Fetches and parses feeds in parallel.
fn fetch_parallel(
    targets: &[SyncTarget],
    config: &AppConfig,
    rules: Arc<Vec<CompiledRule>>,
) -> Result<(Vec<SyncResult>, Vec<SyncError>), AppError> {
    let workers = config.sync.parallel.max(1);
    let (job_tx, job_rx) = mpsc::channel::<SyncTarget>();
    let (result_tx, result_rx) = mpsc::channel::<WorkerResult>();
    let shared_rx = Arc::new(Mutex::new(job_rx));

    let mut handles = Vec::new();
    for _ in 0..workers {
        let rx = Arc::clone(&shared_rx);
        let tx = result_tx.clone();
        let config = config.clone();
        let rules = Arc::clone(&rules);
        let handle = thread::spawn(move || {
            loop {
                let job = {
                    let guard = rx.lock().expect("lock job rx");
                    guard.recv()
                };
                match job {
                    Ok(target) => {
                        let result = fetch_and_parse(&target, &config, &rules);
                        let _ = tx.send(result);
                    }
                    Err(_) => break,
                }
            }
        });
        handles.push(handle);
    }

    for target in targets {
        job_tx
            .send(target.clone())
            .map_err(|error| AppError::io(format!("Failed to send job: {error}")))?;
    }
    drop(job_tx);
    drop(result_tx);

    let mut results = Vec::new();
    let mut errors = Vec::new();
    let mut fatal: Option<AppError> = None;
    for _ in 0..targets.len() {
        let result = result_rx
            .recv()
            .map_err(|error| AppError::io(format!("Failed to receive result: {error}")))?;
        match result {
            WorkerResult::Ok(parsed) => results.push(parsed),
            WorkerResult::Error(error) => errors.push(error),
            WorkerResult::Fatal(error) => fatal = Some(error),
        }
    }

    for handle in handles {
        let _ = handle.join();
    }

    if let Some(error) = fatal {
        return Err(error);
    }
    Ok((results, errors))
}

/// Fetches a single feed and parses entries.
fn fetch_and_parse(
    target: &SyncTarget,
    config: &AppConfig,
    rules: &[CompiledRule],
) -> WorkerResult {
    let bytes = match fetch_feed_bytes(&target.url, &config.sync) {
        Ok(bytes) => bytes,
        Err(error) => return WorkerResult::Error(SyncError::fetch(&target.url, error.to_string())),
    };
    let feed = match feed_rs::parser::parse(Cursor::new(bytes)) {
        Ok(feed) => feed,
        Err(error) => return WorkerResult::Error(SyncError::parse(&target.url, error.to_string())),
    };
    let entries = match feed
        .entries
        .iter()
        .map(|entry| normalize_entry(entry, target, rules, config))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(entries) => entries,
        Err(error) => return WorkerResult::Fatal(error),
    };
    WorkerResult::Ok(SyncResult { entries })
}

/// Normalizes a feed entry into database payloads.
fn normalize_entry(
    entry: &feed_rs::model::Entry,
    target: &SyncTarget,
    rules: &[CompiledRule],
    config: &AppConfig,
) -> Result<SyncEntry, AppError> {
    let source_id = if entry.id.is_empty() {
        None
    } else {
        Some(entry.id.clone())
    };
    let link = entry.links.first().map(|link| link.href.clone());
    let title = entry.title.as_ref().map(|title| title.content.clone());
    let author = entry.authors.first().map(|author| author.name.clone());
    let published_at = entry.published.map(|value| value.timestamp());
    let updated_at = entry.updated.map(|value| value.timestamp());
    let first_seen_at = current_epoch();

    let entry_key = build_entry_key(
        &target.feed_key,
        source_id.as_ref(),
        link.as_ref(),
        title.as_ref(),
    );

    let (content, content_type) = select_content(entry);
    let content_input = build_entry_content(config, content, content_type)?;

    let mut tags = Vec::new();
    tags.extend(target.tags.iter().cloned());
    let title_value = title.clone().unwrap_or_default();
    tags.extend(match_auto_tags(&title_value, rules));
    tags.push(config.unread_tag.clone());
    let tags = dedupe_tags(tags);

    Ok(SyncEntry {
        entry: EntryInput {
            entry_key,
            feed_id: target.feed_id,
            source_id,
            link,
            title,
            author,
            published_at,
            updated_at,
            first_seen_at,
            meta_json: None,
        },
        content: content_input,
        tags,
    })
}

/// Fetches raw feed bytes with retry support.
fn fetch_feed_bytes(url: &str, sync: &crate::config::SyncConfig) -> Result<Vec<u8>, AppError> {
    if let Some(path) = url.strip_prefix("file://") {
        return fs::read(path)
            .map_err(|error| AppError::io(format!("Failed to read feed file {url}: {error}")));
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_read(Duration::from_secs(sync.timeout_secs))
        .timeout_write(Duration::from_secs(sync.timeout_secs))
        .build();
    for attempt in 0..=sync.retry_count {
        let response = agent.get(url).set("User-Agent", &sync.user_agent).call();
        match response {
            Ok(response) => {
                let mut bytes = Vec::new();
                let mut reader = response.into_reader();
                reader
                    .read_to_end(&mut bytes)
                    .map_err(|error| AppError::io(format!("Failed to read feed body: {error}")))?;
                return Ok(bytes);
            }
            Err(error) => {
                if attempt >= sync.retry_count {
                    return Err(AppError::io(format!("Failed to fetch {url}: {error}")));
                }
                if sync.retry_delay_secs > 0 {
                    thread::sleep(Duration::from_secs(sync.retry_delay_secs));
                }
            }
        }
    }
    Err(AppError::io(format!("Failed to fetch {url}")))
}

/// Selects the best content payload from a feed entry.
fn select_content(entry: &feed_rs::model::Entry) -> (Option<String>, Option<String>) {
    if let Some(content) = &entry.content {
        if let Some(body) = &content.body {
            return (Some(body.clone()), Some(content.content_type.to_string()));
        }
    }
    if let Some(summary) = &entry.summary {
        return (
            Some(summary.content.clone()),
            Some(summary.content_type.to_string()),
        );
    }
    (None, None)
}

/// Builds entry content payload according to storage config.
fn build_entry_content(
    config: &AppConfig,
    content: Option<String>,
    content_type: Option<String>,
) -> Result<Option<EntryContentInput>, AppError> {
    let Some(content) = content else {
        return Ok(Some(EntryContentInput {
            storage: "none".to_string(),
            reference: None,
            content_type,
            content: None,
        }));
    };
    match config.storage.content_store {
        ContentStore::Db => Ok(Some(EntryContentInput {
            storage: "db".to_string(),
            reference: None,
            content_type,
            content: Some(content),
        })),
        ContentStore::Fs => {
            let reference = store_content_fs(&config.storage.data_dir, &content)?;
            Ok(Some(EntryContentInput {
                storage: "fs".to_string(),
                reference: Some(reference),
                content_type,
                content: None,
            }))
        }
        ContentStore::None => Ok(Some(EntryContentInput {
            storage: "none".to_string(),
            reference: None,
            content_type,
            content: None,
        })),
    }
}

/// Stores content on filesystem and returns the hash reference.
fn store_content_fs(root: &std::path::Path, content: &str) -> Result<String, AppError> {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(digest);
    let (prefix, _) = hex.split_at(2);
    let dir = root.join(prefix);
    fs::create_dir_all(&dir)
        .map_err(|error| AppError::io(format!("Failed to create content dir: {error}")))?;
    let path = dir.join(&hex);
    fs::write(&path, content.as_bytes())
        .map_err(|error| AppError::io(format!("Failed to write content: {error}")))?;
    Ok(hex)
}

/// Builds a stable entry key from feed key and identifiers.
fn build_entry_key(
    feed_key: &str,
    source_id: Option<&String>,
    link: Option<&String>,
    title: Option<&String>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(feed_key.as_bytes());
    if let Some(source_id) = source_id {
        hasher.update(b":id:");
        hasher.update(source_id.as_bytes());
    } else if let Some(link) = link {
        hasher.update(b":link:");
        hasher.update(link.as_bytes());
    } else if let Some(title) = title {
        hasher.update(b":title:");
        hasher.update(title.as_bytes());
    } else {
        hasher.update(b":fallback:");
        hasher.update(current_epoch().to_string().as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Compiles auto-tag rules into matchable structures.
fn compile_auto_tags(rules: &[AutoTagRule]) -> Result<Vec<CompiledRule>, AppError> {
    let mut compiled = Vec::new();
    for rule in rules {
        let regex = match &rule.title_regex {
            Some(pattern) => Some(
                Regex::new(pattern)
                    .map_err(|error| AppError::config(format!("Invalid title_regex: {error}")))?,
            ),
            None => None,
        };
        let contains = rule
            .title_contains
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| value.to_lowercase())
            .collect();
        compiled.push(CompiledRule {
            regex,
            contains,
            add_tags: rule.add_tags.clone(),
            priority: rule.priority.unwrap_or(0),
        });
    }
    compiled.sort_by_key(|rule| rule.priority);
    Ok(compiled)
}

/// Evaluates auto-tag rules against an entry title.
fn match_auto_tags(title: &str, rules: &[CompiledRule]) -> Vec<String> {
    let lower = title.to_lowercase();
    let mut tags = Vec::new();
    for rule in rules {
        let mut matched = false;
        if let Some(regex) = &rule.regex {
            matched |= regex.is_match(title);
        }
        if !rule.contains.is_empty() {
            matched |= rule.contains.iter().any(|token| lower.contains(token));
        }
        if matched {
            tags.extend(rule.add_tags.iter().cloned());
        }
    }
    tags
}

/// Deduplicates tags while preserving order.
fn dedupe_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for tag in tags {
        if seen.insert(tag.clone()) {
            out.push(tag);
        }
    }
    out
}

/// Returns current epoch seconds.
fn current_epoch() -> i64 {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
    duration.as_secs() as i64
}

impl SyncError {
    /// Builds a fetch error entry.
    fn fetch(feed_url: &str, message: String) -> Self {
        Self {
            feed_url: feed_url.to_string(),
            code: "FETCH_FAILED".to_string(),
            message,
            retry: !feed_url.starts_with("file://"),
        }
    }

    /// Builds a parse error entry.
    fn parse(feed_url: &str, message: String) -> Self {
        Self {
            feed_url: feed_url.to_string(),
            code: "PARSE_FAILED".to_string(),
            message,
            retry: false,
        }
    }
}
