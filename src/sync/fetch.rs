//! Feed fetch pipeline.

use crate::config::{AppConfig, SyncConfig};
use crate::error::AppError;
use crossbeam_channel::{Receiver, Sender, select, unbounded};
use std::fs;
use std::io::{Cursor, Read};
use std::thread;
use std::time::Duration;

use super::model::{SyncError, SyncProgressEvent, SyncResult, SyncTarget, WorkerResult};
use super::normalize::normalize_entry;

/// Feed fetch failure with retryability metadata.
struct FetchError {
    message: String,
    retryable: bool,
}

/// Fetches and parses feeds in parallel.
pub(crate) fn fetch_parallel(
    targets: &[SyncTarget],
    config: &AppConfig,
    mut on_progress: Option<&mut dyn FnMut(SyncProgressEvent)>,
) -> Result<(Vec<SyncResult>, Vec<SyncError>), AppError> {
    let workers = config.sync.parallel.max(1);
    let (job_tx, job_rx) = unbounded::<SyncTarget>();
    let (result_tx, result_rx) = unbounded::<WorkerResult>();
    let (cancel_tx, cancel_rx) = unbounded::<()>();

    let mut handles = Vec::new();
    for _ in 0..workers {
        let job_rx = job_rx.clone();
        let cancel_rx = cancel_rx.clone();
        let tx = result_tx.clone();
        let config = config.clone();
        let handle = thread::spawn(move || worker_loop(job_rx, cancel_rx, tx, &config));
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
    loop {
        let result = match result_rx.recv() {
            Ok(result) => result,
            Err(_) => break,
        };
        match result {
            WorkerResult::Started {
                index,
                total_feeds,
                url,
            } => {
                if let Some(progress) = on_progress.as_mut() {
                    progress(SyncProgressEvent::FeedStart {
                        index,
                        total_feeds,
                        url,
                    });
                }
            }
            WorkerResult::Ok {
                index,
                total_feeds,
                url,
                result,
            } => {
                if let Some(progress) = on_progress.as_mut() {
                    progress(SyncProgressEvent::FeedOk {
                        index,
                        total_feeds,
                        url,
                        entries: result.entries.len(),
                    });
                }
                results.push(result);
            }
            WorkerResult::Error {
                index,
                total_feeds,
                url,
                error,
            } => {
                if let Some(progress) = on_progress.as_mut() {
                    progress(SyncProgressEvent::FeedError {
                        index,
                        total_feeds,
                        url,
                        code: error.code,
                        retryable: error.retryable,
                    });
                }
                errors.push(error);
            }
            WorkerResult::Fatal(error) => {
                fatal = Some(error);
                drop(cancel_tx);
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

/// Worker loop that consumes sync targets and reports results.
fn worker_loop(
    job_rx: Receiver<SyncTarget>,
    cancel_rx: Receiver<()>,
    result_tx: Sender<WorkerResult>,
    config: &AppConfig,
) {
    loop {
        select! {
            recv(cancel_rx) -> _ => break,
            recv(job_rx) -> job => match job {
                Ok(target) => {
                    let _ = result_tx.send(WorkerResult::Started {
                        index: target.index,
                        total_feeds: target.total_feeds,
                        url: target.url.clone(),
                    });
                    let result = fetch_and_parse(&target, config);
                    let _ = result_tx.send(result);
                }
                Err(_) => break,
            }
        }
    }
}

/// Fetches a single feed and parses entries.
fn fetch_and_parse(target: &SyncTarget, config: &AppConfig) -> WorkerResult {
    let bytes = match fetch_feed_bytes(&target.url, &config.sync) {
        Ok(bytes) => bytes,
        Err(error) => {
            return WorkerResult::Error {
                index: target.index,
                total_feeds: target.total_feeds,
                url: target.url.clone(),
                error: SyncError::fetch(&target.url, error.message, error.retryable),
            };
        }
    };
    let feed = match feed_rs::parser::parse(Cursor::new(bytes)) {
        Ok(feed) => feed,
        Err(error) => {
            return WorkerResult::Error {
                index: target.index,
                total_feeds: target.total_feeds,
                url: target.url.clone(),
                error: SyncError::parse(&target.url, error.to_string()),
            };
        }
    };
    let entries = match feed
        .entries
        .iter()
        .map(|entry| normalize_entry(entry, target, config))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(entries) => entries,
        Err(error) => return WorkerResult::Fatal(error),
    };
    WorkerResult::Ok {
        index: target.index,
        total_feeds: target.total_feeds,
        url: target.url.clone(),
        result: SyncResult { entries },
    }
}

/// Fetches raw feed bytes with retry support.
fn fetch_feed_bytes(url: &str, sync: &SyncConfig) -> Result<Vec<u8>, FetchError> {
    if let Some(path) = url.strip_prefix("file://") {
        return fs::read(path).map_err(|error| FetchError {
            message: format!("Failed to read feed file {url}: {error}"),
            retryable: false,
        });
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
                reader.read_to_end(&mut bytes).map_err(|error| FetchError {
                    message: format!("Failed to read feed body: {error}"),
                    retryable: true,
                })?;
                return Ok(bytes);
            }
            Err(error) => {
                if let ureq::Error::Status(code, _) = &error
                    && (400..500).contains(code)
                {
                    return Err(FetchError {
                        message: format!("Failed to fetch {url}: {error}"),
                        retryable: false,
                    });
                }
                if attempt >= sync.retry_count {
                    return Err(FetchError {
                        message: format!("Failed to fetch {url}: {error}"),
                        retryable: true,
                    });
                }
                if sync.retry_delay_secs > 0 {
                    thread::sleep(Duration::from_secs(sync.retry_delay_secs));
                }
            }
        }
    }
    Err(FetchError {
        message: format!("Failed to fetch {url}"),
        retryable: true,
    })
}
