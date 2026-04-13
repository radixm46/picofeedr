//! Feed fetch pipeline.

use crate::config::{AppConfig, SyncConfig};
use crate::error::AppError;
use crossbeam_channel::{Receiver, Sender, bounded, select, unbounded};
use std::fs;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use url::Url;

use super::model::{
    FeedMetadata, SyncError, SyncProgressEvent, SyncResult, SyncTarget, WorkerResult,
};
use super::normalize::normalize_entry;

/// Feed fetch failure with retryability metadata.
#[derive(Debug)]
struct FetchError {
    message: String,
    retryable: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum FeedSource {
    Http(String),
    File(PathBuf),
}

/// Fetches and parses feeds in parallel.
pub(crate) fn fetch_parallel<F>(
    targets: &[SyncTarget],
    config: &AppConfig,
    mut on_progress: Option<&mut dyn FnMut(SyncProgressEvent)>,
    mut on_result: F,
) -> Result<Vec<SyncError>, AppError>
where
    F: FnMut(SyncResult) -> Result<(), SyncError>,
{
    let workers = config.sync.parallel.max(1);
    let (job_tx, job_rx) = unbounded::<SyncTarget>();
    let (result_tx, result_rx) = bounded::<WorkerResult>(workers * 2);
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
                if let Err(error) = on_result(result) {
                    errors.push(error);
                }
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
    Ok(errors)
}

/// Worker loop that consumes sync targets and reports results.
fn worker_loop(
    job_rx: Receiver<SyncTarget>,
    cancel_rx: Receiver<()>,
    result_tx: Sender<WorkerResult>,
    config: &AppConfig,
) {
    let agent = build_agent(&config.sync);
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
                    let result = fetch_and_parse(&target, config, &agent);
                    let _ = result_tx.send(result);
                }
                Err(_) => break,
            }
        }
    }
}

/// Fetches a single feed and parses entries.
fn fetch_and_parse(target: &SyncTarget, config: &AppConfig, agent: &ureq::Agent) -> WorkerResult {
    let bytes = match fetch_feed_bytes(&target.url, &config.sync, agent) {
        Ok(bytes) => bytes,
        Err(error) => {
            return WorkerResult::Error {
                index: target.index,
                total_feeds: target.total_feeds,
                url: target.url.clone(),
                error: SyncError::fetch(
                    &target.feed_id,
                    target.feed_name.as_deref(),
                    &target.url,
                    target.index,
                    target.total_feeds,
                    error.message,
                    error.retryable,
                ),
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
                error: SyncError::parse(
                    &target.feed_id,
                    target.feed_name.as_deref(),
                    &target.url,
                    target.index,
                    target.total_feeds,
                    error.to_string(),
                ),
            };
        }
    };
    let feed_metadata = extract_feed_metadata(&feed);
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
        result: SyncResult {
            feed_id: target.feed_id.clone(),
            feed_name: target.feed_name.clone(),
            feed_url: target.url.clone(),
            index: target.index,
            total_feeds: target.total_feeds,
            feed_metadata,
            entries,
        },
    }
}

fn extract_feed_metadata(feed: &feed_rs::model::Feed) -> FeedMetadata {
    FeedMetadata {
        title: trim_to_option(feed.title.as_ref().map(|title| title.content.as_str())),
        author: trim_to_option(feed.authors.first().map(|author| author.name.as_str())),
        site_url: extract_site_url(feed),
    }
}

fn extract_site_url(feed: &feed_rs::model::Feed) -> Option<String> {
    feed.links
        .iter()
        .find(|link| {
            trim_str_to_option(link.href.as_str()).is_some()
                && link.rel.as_deref() == Some("alternate")
        })
        .or_else(|| {
            feed.links.iter().find(|link| {
                trim_str_to_option(link.href.as_str()).is_some()
                    && !matches!(link.rel.as_deref(), Some("self"))
            })
        })
        .and_then(|link| trim_str_to_option(link.href.as_str()))
}

fn trim_to_option(value: Option<&str>) -> Option<String> {
    value.and_then(trim_str_to_option)
}

fn trim_str_to_option(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn build_agent(sync: &SyncConfig) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(sync.timeout_secs)))
        .timeout_send_request(Some(Duration::from_secs(sync.timeout_secs)))
        .timeout_send_body(Some(Duration::from_secs(sync.timeout_secs)))
        .timeout_recv_body(Some(Duration::from_secs(sync.timeout_secs)))
        .build()
        .into()
}

fn read_limited_bytes<R: Read>(
    reader: R,
    max_feed_bytes: usize,
    read_error_prefix: &str,
    retryable: bool,
) -> Result<Vec<u8>, FetchError> {
    let mut bytes = Vec::new();
    let limit = max_feed_bytes.saturating_add(1) as u64;
    let mut limited = reader.take(limit);
    limited
        .read_to_end(&mut bytes)
        .map_err(|error| FetchError {
            message: format!("{read_error_prefix}: {error}"),
            retryable,
        })?;
    if bytes.len() > max_feed_bytes {
        return Err(FetchError {
            message: "Feed body exceeds max_feed_bytes".to_string(),
            retryable: false,
        });
    }
    Ok(bytes)
}

fn read_feed_file(path: &std::path::Path, sync: &SyncConfig) -> Result<Vec<u8>, FetchError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() > sync.max_feed_bytes as u64 => {
            return Err(FetchError {
                message: "Feed body exceeds max_feed_bytes".to_string(),
                retryable: false,
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(FetchError {
                message: format!("Failed to read feed file: {error}"),
                retryable: false,
            });
        }
        // Metadata size check is best-effort; let File::open surface the real read error.
        Err(_) => {}
    }

    let file = fs::File::open(path).map_err(|error| FetchError {
        message: format!("Failed to read feed file: {error}"),
        retryable: false,
    })?;
    read_limited_bytes(file, sync.max_feed_bytes, "Failed to read feed file", false)
}

fn parse_feed_source(url: &str) -> Result<FeedSource, FetchError> {
    let parsed = Url::parse(url).map_err(|error| FetchError {
        message: format!("Invalid feed URL: {error}"),
        retryable: false,
    })?;
    match parsed.scheme() {
        "http" | "https" => Ok(FeedSource::Http(url.to_string())),
        "file" => {
            let path = parsed.to_file_path().map_err(|_| FetchError {
                message: "Invalid file URL".to_string(),
                retryable: false,
            })?;
            Ok(FeedSource::File(path))
        }
        scheme => Err(FetchError {
            message: format!("Unsupported feed URL scheme: {scheme}"),
            retryable: false,
        }),
    }
}

/// Fetches raw feed bytes with retry support.
fn fetch_feed_bytes(
    url: &str,
    sync: &SyncConfig,
    agent: &ureq::Agent,
) -> Result<Vec<u8>, FetchError> {
    match parse_feed_source(url)? {
        FeedSource::File(path) => return read_feed_file(&path, sync),
        FeedSource::Http(parsed_url) => {
            for attempt in 0..=sync.retry_count {
                let response = agent
                    .get(&parsed_url)
                    .header("User-Agent", &sync.user_agent)
                    .call();
                match response {
                    Ok(mut response) => {
                        return read_limited_bytes(
                            response.body_mut().as_reader(),
                            sync.max_feed_bytes,
                            "Failed to read feed body",
                            true,
                        );
                    }
                    Err(error) => {
                        if matches!(&error, ureq::Error::StatusCode(code) if (400..500).contains(code))
                        {
                            return Err(FetchError {
                                message: trim_url_prefix(&parsed_url, error.to_string()),
                                retryable: false,
                            });
                        }
                        if attempt >= sync.retry_count {
                            return Err(FetchError {
                                message: trim_url_prefix(&parsed_url, error.to_string()),
                                retryable: true,
                            });
                        }
                        if sync.retry_delay_secs > 0 {
                            thread::sleep(Duration::from_secs(sync.retry_delay_secs));
                        }
                    }
                }
            }
        }
    }
    Err(FetchError {
        message: "Fetch failed".to_string(),
        retryable: true,
    })
}

fn trim_url_prefix(url: &str, message: String) -> String {
    let prefix = format!("{url}: ");
    message
        .strip_prefix(&prefix)
        .map(ToOwned::to_owned)
        .unwrap_or(message)
}

#[cfg(test)]
mod tests {
    use super::{FeedSource, build_agent, fetch_feed_bytes, parse_feed_source};
    use crate::config::SyncConfig;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::thread;
    use tempfile::TempDir;
    use url::Url;

    fn test_sync_config() -> SyncConfig {
        SyncConfig {
            parallel: 1,
            timeout_secs: 2,
            user_agent: "picofeedr-test".to_string(),
            retry_count: 0,
            retry_delay_secs: 0,
            max_feed_bytes: 2 * 1024 * 1024,
        }
    }

    fn spawn_http_body_server(body: &'static [u8]) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).expect("write headers");
            stream.write_all(body).expect("write body");
        });
        (format!("http://{addr}/feed.xml"), server)
    }

    #[test]
    fn parse_feed_source_parses_http_url() {
        let source = parse_feed_source("https://example.com/feed.xml").expect("source");
        assert_eq!(
            source,
            FeedSource::Http("https://example.com/feed.xml".to_string())
        );
    }

    #[test]
    fn parse_feed_source_parses_file_url() {
        let path = PathBuf::from("/tmp/feed.xml");
        let source = parse_feed_source("file:///tmp/feed.xml").expect("source");
        assert_eq!(source, FeedSource::File(path));
    }

    #[test]
    fn parse_feed_source_rejects_unsupported_scheme() {
        let error = parse_feed_source("ftp://example.com/feed.xml").expect_err("error");
        assert!(!error.retryable);
        assert_eq!(error.message, "Unsupported feed URL scheme: ftp");
    }

    #[test]
    fn fetch_feed_bytes_reads_file_url() {
        let temp = TempDir::new().expect("temp dir");
        let feed_path = temp.path().join("feed.xml");
        fs::write(&feed_path, "<rss></rss>").expect("write feed");
        let url = Url::from_file_path(&feed_path)
            .expect("file url")
            .to_string();
        let sync = test_sync_config();
        let agent = build_agent(&sync);

        let bytes = fetch_feed_bytes(&url, &sync, &agent).expect("fetch file");

        assert_eq!(bytes, b"<rss></rss>");
    }

    #[test]
    fn fetch_feed_bytes_reads_http_body_within_limit() {
        let body = b"<rss></rss>";
        let (url, server) = spawn_http_body_server(body);
        let sync = test_sync_config();
        let agent = build_agent(&sync);

        let bytes = fetch_feed_bytes(&url, &sync, &agent).expect("fetch http");
        server.join().expect("server join");

        assert_eq!(bytes, body);
    }

    #[test]
    fn fetch_feed_bytes_rejects_oversized_http_body() {
        let body = b"<rss>123456789</rss>";
        let (url, server) = spawn_http_body_server(body);
        let mut sync = test_sync_config();
        sync.max_feed_bytes = 8;
        let agent = build_agent(&sync);

        let error = fetch_feed_bytes(&url, &sync, &agent).expect_err("expect oversize error");
        server.join().expect("server join");

        assert!(!error.retryable);
        assert_eq!(error.message, "Feed body exceeds max_feed_bytes");
    }

    #[test]
    fn fetch_feed_bytes_rejects_oversized_file_url() {
        let temp = TempDir::new().expect("temp dir");
        let feed_path = temp.path().join("feed.xml");
        fs::write(&feed_path, "<rss>123456789</rss>").expect("write feed");
        let url = Url::from_file_path(&feed_path)
            .expect("file url")
            .to_string();
        let mut sync = test_sync_config();
        sync.max_feed_bytes = 8;
        let agent = build_agent(&sync);

        let error = fetch_feed_bytes(&url, &sync, &agent).expect_err("expect oversize error");

        assert!(!error.retryable);
        assert_eq!(error.message, "Feed body exceeds max_feed_bytes");
    }

    #[test]
    fn fetch_feed_bytes_rejects_invalid_file_url() {
        let sync = test_sync_config();
        let agent = build_agent(&sync);

        let error =
            fetch_feed_bytes("file://example.com/feed.xml", &sync, &agent).expect_err("error");

        assert!(!error.retryable);
        assert_eq!(error.message, "Invalid file URL");
    }

    #[test]
    fn fetch_feed_bytes_rejects_unsupported_scheme() {
        let sync = test_sync_config();
        let agent = build_agent(&sync);

        let error =
            fetch_feed_bytes("ftp://example.com/feed.xml", &sync, &agent).expect_err("error");

        assert!(!error.retryable);
        assert_eq!(error.message, "Unsupported feed URL scheme: ftp");
    }

    #[test]
    fn fetch_feed_bytes_404_is_not_retryable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("write response");
        });

        let url = format!("http://{addr}/missing");
        let sync = test_sync_config();
        let agent = build_agent(&sync);

        let error = fetch_feed_bytes(&url, &sync, &agent).expect_err("expect 404 error");
        server.join().expect("server join");

        assert!(!error.retryable);
    }
}
