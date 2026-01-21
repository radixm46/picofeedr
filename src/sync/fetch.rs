//! Feed fetch pipeline.

use crate::config::{AppConfig, SyncConfig};
use crate::error::AppError;
use std::fs;
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use super::autotag::CompiledRule;
use super::model::{SyncError, SyncResult, SyncTarget, WorkerResult};
use super::normalize::normalize_entry;

/// Fetches and parses feeds in parallel.
pub(crate) fn fetch_parallel(
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
                    let guard = match rx.lock() {
                        Ok(guard) => guard,
                        Err(_) => {
                            let _ = tx.send(WorkerResult::Fatal(AppError::io(
                                "Worker queue lock poisoned",
                            )));
                            break;
                        }
                    };
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
    let mut received = 0usize;
    while received < targets.len() {
        let result = result_rx
            .recv()
            .map_err(|error| AppError::io(format!("Failed to receive result: {error}")))?;
        received += 1;
        match result {
            WorkerResult::Ok(parsed) => results.push(parsed),
            WorkerResult::Error(error) => errors.push(error),
            WorkerResult::Fatal(error) => {
                fatal = Some(error);
                break;
            }
        }
    }

    for handle in handles {
        if handle.join().is_err() {
            fatal = Some(AppError::io("Worker panicked"));
        }
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

/// Fetches raw feed bytes with retry support.
fn fetch_feed_bytes(url: &str, sync: &SyncConfig) -> Result<Vec<u8>, AppError> {
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
