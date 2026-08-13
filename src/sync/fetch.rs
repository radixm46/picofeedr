//! Feed fetch pipeline.

use crate::config::{AppConfig, SyncConfig};
use crate::error::AppError;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use std::fs;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use url::Url;

use super::gopher_transport::fetch_gopher_bytes;
use super::model::{
    FeedMetadata, SyncError, SyncProgressEvent, SyncResult, SyncTarget, WorkerResult,
};
use super::normalize::normalize_entry;

/// Feed fetch failure with retryability metadata.
#[derive(Debug)]
pub(super) struct FetchError {
    pub(super) message: String,
    pub(super) retryable: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum FeedSource {
    Http(String),
    File(PathBuf),
    Gopher(String),
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

    let mut handles = Vec::new();
    for _ in 0..workers {
        let job_rx = job_rx.clone();
        let tx = result_tx.clone();
        let config = config.clone();
        let handle = thread::spawn(move || worker_loop(job_rx, tx, &config));
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
    let mut completed_feeds = 0usize;
    while let Ok(result) = result_rx.recv() {
        match result {
            WorkerResult::Started { ctx } => {
                if let Some(progress) = on_progress.as_mut() {
                    progress(SyncProgressEvent::FeedStart { url: ctx.url });
                }
            }
            WorkerResult::Ok { ctx, result } => {
                completed_feeds += 1;
                let entries = result.entries.len();
                match on_result(result) {
                    Ok(()) => {
                        if let Some(progress) = on_progress.as_mut() {
                            progress(SyncProgressEvent::FeedOk {
                                index: completed_feeds,
                                total_feeds: targets.len(),
                                url: ctx.url,
                                entries,
                            });
                        }
                    }
                    Err(error) => {
                        if let Some(progress) = on_progress.as_mut() {
                            progress(SyncProgressEvent::FeedError {
                                url: ctx.url,
                                code: error.code,
                                retryable: error.retryable,
                            });
                        }
                        errors.push(error);
                    }
                }
            }
            WorkerResult::Error { ctx, error } => {
                completed_feeds += 1;
                if let Some(progress) = on_progress.as_mut() {
                    progress(SyncProgressEvent::FeedError {
                        url: ctx.url,
                        code: error.code,
                        retryable: error.retryable,
                    });
                }
                errors.push(error);
            }
        }
    }

    drop(result_rx);
    for handle in handles {
        if handle.join().is_err() {
            fatal = Some(AppError::internal("Worker panicked"));
        }
    }

    if let Some(error) = fatal {
        return Err(error);
    }
    Ok(errors)
}

/// Worker loop that consumes sync targets and reports results.
fn worker_loop(job_rx: Receiver<SyncTarget>, result_tx: Sender<WorkerResult>, config: &AppConfig) {
    let agent = build_agent(&config.sync);
    while let Ok(target) = job_rx.recv() {
        if result_tx
            .send(WorkerResult::Started {
                ctx: target.ctx.clone(),
            })
            .is_err()
        {
            break;
        }
        let result = fetch_and_parse(&target, config, &agent);
        if result_tx.send(result).is_err() {
            break;
        }
    }
}

/// Fetches a single feed and parses entries.
fn fetch_and_parse(target: &SyncTarget, config: &AppConfig, agent: &ureq::Agent) -> WorkerResult {
    let bytes = match fetch_feed_bytes(&target.ctx.url, &config.sync, agent) {
        Ok(bytes) => bytes,
        Err(error) => {
            return WorkerResult::Error {
                ctx: target.ctx.clone(),
                error: SyncError::fetch(&target.ctx, error.message, error.retryable),
            };
        }
    };
    let feed = match feed_rs::parser::parse(Cursor::new(bytes)) {
        Ok(feed) => feed,
        Err(error) => {
            return WorkerResult::Error {
                ctx: target.ctx.clone(),
                error: SyncError::parse(&target.ctx, error.to_string()),
            };
        }
    };
    let feed_metadata = extract_feed_metadata(&feed);
    let entries = feed
        .entries
        .iter()
        .map(|entry| normalize_entry(entry, target, config))
        .collect();
    let ctx = target.ctx.clone();
    WorkerResult::Ok {
        ctx: ctx.clone(),
        result: SyncResult {
            ctx,
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
        "gopher" => Ok(FeedSource::Gopher(url.to_string())),
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
        FeedSource::File(path) => read_feed_file(&path, sync),
        FeedSource::Gopher(parsed_url) => fetch_gopher_bytes(&parsed_url, sync),
        FeedSource::Http(parsed_url) => {
            let mut attempt = 0;
            loop {
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
                        attempt += 1;
                    }
                }
            }
        }
    }
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
    use super::{
        FeedSource, WorkerResult, build_agent, fetch_feed_bytes, fetch_parallel, parse_feed_source,
        worker_loop,
    };
    use crate::config::{
        AppConfig, CliConfig, ContentStore, DatabaseConfig, FeedsSourceConfig, QueryConfig,
        StorageConfig, SyncConfig,
    };
    use crate::sync::model::{
        FeedContext, SyncError, SyncErrorCode, SyncProgressEvent, SyncTarget,
    };
    use crossbeam_channel::{bounded, unbounded};
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver};
    use std::thread;
    use std::time::Duration;
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

    fn test_app_config() -> AppConfig {
        AppConfig {
            manage_unread: false,
            unread_tag: "unread".to_string(),
            database: DatabaseConfig {
                path: PathBuf::from("/tmp/db.sqlite"),
            },
            feeds: FeedsSourceConfig {
                source: PathBuf::from("/tmp/feeds.yaml"),
            },
            sync: test_sync_config(),
            storage: StorageConfig {
                root_dir: PathBuf::from("/tmp/root"),
                content_store: ContentStore::None,
                data_dir: PathBuf::from("/tmp/root/data"),
            },
            query: QueryConfig {
                default_limit: 100,
                max_limit: 1000,
            },
            cli: CliConfig {
                output: crate::cli::OutputFormat::Plain,
            },
        }
    }

    fn test_sync_target() -> SyncTarget {
        SyncTarget {
            ctx: FeedContext {
                feed_id: "test-feed".to_string(),
                feed_name: None,
                url: "file:///definitely/missing/feed.xml".to_string(),
            },
            tags: Vec::new(),
            auto_tag_rules: Vec::new(),
        }
    }

    #[test]
    fn fetch_success_with_ingest_failure_reports_feed_error_without_feed_ok() {
        let temp = TempDir::new().expect("temp dir");
        let write_feed = |path: &std::path::Path, entry_id: &str| {
            fs::write(
                path,
                format!(
                    r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0"><channel><title>Test</title><link>https://example.com</link><description>Test</description>
<item><guid>{entry_id}</guid><title>Entry</title><link>https://example.com/{entry_id}</link><description>Body</description></item>
</channel></rss>"#
                ),
            )
            .expect("write feed");
        };
        let first_feed_path = temp.path().join("first-feed.xml");
        let second_feed_path = temp.path().join("second-feed.xml");
        write_feed(&first_feed_path, "entry-1");
        write_feed(&second_feed_path, "entry-2");
        let target_for = |feed_id: &str, path: &std::path::Path| SyncTarget {
            ctx: FeedContext {
                feed_id: feed_id.to_string(),
                feed_name: None,
                url: Url::from_file_path(path).expect("file url").to_string(),
            },
            tags: Vec::new(),
            auto_tag_rules: Vec::new(),
        };
        let targets = vec![
            target_for("test-feed-1", &first_feed_path),
            target_for("test-feed-2", &second_feed_path),
        ];
        let mut config = test_app_config();
        config.sync.parallel = 1;
        let mut events = Vec::new();
        let mut on_progress = |event| events.push(event);
        let mut result_count = 0;

        let errors = fetch_parallel(&targets, &config, Some(&mut on_progress), |result| {
            result_count += 1;
            if result_count == 1 {
                Err(SyncError::ingest(
                    &result.ctx,
                    "forced ingest failure".to_string(),
                ))
            } else {
                Ok(())
            }
        })
        .expect("fetch completes");

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, SyncErrorCode::IngestFailed);
        assert_eq!(errors[0].feed_id, "test-feed-1");
        assert_eq!(events.len(), 4);
        assert!(matches!(
            &events[0],
            SyncProgressEvent::FeedStart { url } if url == &targets[0].ctx.url
        ));
        assert!(matches!(
            &events[1],
            SyncProgressEvent::FeedError {
                url,
                code: SyncErrorCode::IngestFailed,
                retryable: false,
            } if url == &targets[0].ctx.url
        ));
        assert!(matches!(
            &events[2],
            SyncProgressEvent::FeedStart { url } if url == &targets[1].ctx.url
        ));
        assert!(matches!(
            &events[3],
            SyncProgressEvent::FeedOk {
                index: 2,
                total_feeds: 2,
                url,
                entries: 1,
            } if url == &targets[1].ctx.url
        ));
    }

    #[test]
    fn worker_stops_when_result_receiver_is_dropped() {
        let (job_tx, job_rx) = unbounded();
        let (result_tx, result_rx) = bounded(0);
        let (done_tx, done_rx) = bounded(1);
        let config = test_app_config();

        let handle = thread::spawn(move || {
            worker_loop(job_rx, result_tx, &config);
            done_tx.send(()).expect("worker completion");
        });
        job_tx.send(test_sync_target()).expect("send job");

        assert!(matches!(
            result_rx.recv().expect("started result"),
            WorkerResult::Started { .. }
        ));
        drop(result_rx);

        let stopped = done_rx.recv_timeout(Duration::from_secs(1)).is_ok();
        handle.join().expect("worker join");
        assert!(
            stopped,
            "worker remained blocked after result receiver closed"
        );
    }

    fn wait_for_server(done_rx: Receiver<()>, label: &str) {
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|error| panic!("{label} did not complete: {error:?}"));
    }

    fn spawn_http_body_server(body: &'static [u8]) -> (String, Receiver<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let (done_tx, done_rx) = mpsc::channel();
        let _server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).expect("write headers");
            stream.write_all(body).expect("write body");
            done_tx.send(()).expect("HTTP server completion");
        });
        (format!("http://{addr}/feed.xml"), done_rx)
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
        wait_for_server(server, "HTTP server");

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
        wait_for_server(server, "HTTP server");

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
        let (done_tx, done_rx) = mpsc::channel();
        let _server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("write response");
            done_tx.send(()).expect("HTTP 404 server completion");
        });

        let url = format!("http://{addr}/missing");
        let sync = test_sync_config();
        let agent = build_agent(&sync);

        let error = fetch_feed_bytes(&url, &sync, &agent).expect_err("expect 404 error");
        wait_for_server(done_rx, "HTTP 404 server");

        assert!(!error.retryable);
    }
}
