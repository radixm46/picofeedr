//! Minimal Gopher transport for fetching feed documents.

use crate::config::SyncConfig;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use url::Url;

use super::fetch::FetchError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GopherItemType {
    Text,
    Directory,
    Binary,
    UnknownSimple,
}

#[derive(Debug, PartialEq, Eq)]
struct GopherRequest {
    host: String,
    port: u16,
    item_type: GopherItemType,
    selector: Vec<u8>,
}

pub(super) fn fetch_gopher_bytes(url: &str, sync: &SyncConfig) -> Result<Vec<u8>, FetchError> {
    let request = parse_gopher_url(url)?;
    let mut attempt = 0;
    loop {
        match fetch_once(&request, sync) {
            Ok(bytes) => return Ok(bytes),
            Err(error) if !error.retryable || attempt >= sync.retry_count => return Err(error),
            Err(_) => {
                if sync.retry_delay_secs > 0 {
                    thread::sleep(Duration::from_secs(sync.retry_delay_secs));
                }
                attempt += 1;
            }
        }
    }
}

fn fetch_once(request: &GopherRequest, sync: &SyncConfig) -> Result<Vec<u8>, FetchError> {
    let addr = format!("{}:{}", request.host, request.port);
    let mut stream = TcpStream::connect(&addr).map_err(|error| FetchError {
        message: format!("Failed to connect to Gopher server: {error}"),
        retryable: true,
    })?;
    let timeout = Some(Duration::from_secs(sync.timeout_secs));
    stream
        .set_read_timeout(timeout)
        .map_err(|error| FetchError {
            message: format!("Failed to configure Gopher read timeout: {error}"),
            retryable: true,
        })?;
    stream
        .set_write_timeout(timeout)
        .map_err(|error| FetchError {
            message: format!("Failed to configure Gopher write timeout: {error}"),
            retryable: true,
        })?;
    stream
        .write_all(&request.selector)
        .map_err(|error| FetchError {
            message: format!("Failed to send Gopher selector: {error}"),
            retryable: true,
        })?;
    stream.write_all(b"\r\n").map_err(|error| FetchError {
        message: format!("Failed to send Gopher selector terminator: {error}"),
        retryable: true,
    })?;
    match request.item_type {
        GopherItemType::Text | GopherItemType::Directory | GopherItemType::UnknownSimple => {
            read_text_response(stream, sync.max_feed_bytes)
        }
        GopherItemType::Binary => read_binary_response(stream, sync.max_feed_bytes),
    }
}

fn read_text_response(stream: TcpStream, max_feed_bytes: usize) -> Result<Vec<u8>, FetchError> {
    let mut reader = BufReader::new(stream);
    let mut body = Vec::new();
    loop {
        let mut line = Vec::new();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| FetchError {
                message: format!("Failed to read Gopher response: {error}"),
                retryable: true,
            })?;
        if read == 0 {
            break;
        }
        if line == b".\r\n" || line == b".\n" || line == b"." {
            break;
        }
        if line.starts_with(b"..") {
            line.remove(0);
        }
        append_with_limit(&mut body, &line, max_feed_bytes)?;
    }
    Ok(body)
}

fn read_binary_response(
    mut stream: TcpStream,
    max_feed_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let mut body = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = stream.read(&mut chunk).map_err(|error| FetchError {
            message: format!("Failed to read Gopher response: {error}"),
            retryable: true,
        })?;
        if read == 0 {
            break;
        }
        append_with_limit(&mut body, &chunk[..read], max_feed_bytes)?;
    }
    Ok(body)
}

fn append_with_limit(
    body: &mut Vec<u8>,
    chunk: &[u8],
    max_feed_bytes: usize,
) -> Result<(), FetchError> {
    if body.len().saturating_add(chunk.len()) > max_feed_bytes {
        return Err(FetchError {
            message: "Feed body exceeds max_feed_bytes".to_string(),
            retryable: false,
        });
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn parse_gopher_url(input: &str) -> Result<GopherRequest, FetchError> {
    let url = Url::parse(input).map_err(|error| FetchError {
        message: format!("Invalid Gopher URL: {error}"),
        retryable: false,
    })?;
    if url.scheme() != "gopher" {
        return Err(FetchError {
            message: "Invalid Gopher URL: scheme must be gopher".to_string(),
            retryable: false,
        });
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(FetchError {
            message: "Unsupported Gopher URL form".to_string(),
            retryable: false,
        });
    }
    let host = url.host_str().ok_or_else(|| FetchError {
        message: "Invalid Gopher URL: missing host".to_string(),
        retryable: false,
    })?;
    let encoded_path = url[url::Position::BeforePath..url::Position::AfterPath].to_string();
    let path = encoded_path
        .strip_prefix('/')
        .unwrap_or(encoded_path.as_str());
    let path_lower = path.to_ascii_lowercase();
    if path_lower.contains("%09") || path.contains('\t') {
        return Err(FetchError {
            message: "Unsupported Gopher URL form: search and Gopher+ are not supported"
                .to_string(),
            retryable: false,
        });
    }
    let (item_type, selector_encoded) = if path.is_empty() {
        (GopherItemType::Directory, "")
    } else {
        let item = path.as_bytes()[0];
        let selector = &path[1..];
        (parse_item_type(item)?, selector)
    };
    let selector = percent_decode(selector_encoded)?;
    Ok(GopherRequest {
        host: host.to_string(),
        port: url.port().unwrap_or(70),
        item_type,
        selector,
    })
}

fn parse_item_type(value: u8) -> Result<GopherItemType, FetchError> {
    match value {
        b'0' => Ok(GopherItemType::Text),
        b'1' => Ok(GopherItemType::Directory),
        b'5' | b'9' => Ok(GopherItemType::Binary),
        b'7' => Err(FetchError {
            message: "Unsupported Gopher URL form: search and Gopher+ are not supported"
                .to_string(),
            retryable: false,
        }),
        b'2' | b'8' => Err(FetchError {
            message: "Unsupported Gopher item type".to_string(),
            retryable: false,
        }),
        _ => Ok(GopherItemType::UnknownSimple),
    }
}

fn percent_decode(input: &str) -> Result<Vec<u8>, FetchError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' {
            if idx + 2 >= bytes.len() {
                return Err(FetchError {
                    message: "Invalid Gopher URL: invalid percent-encoding".to_string(),
                    retryable: false,
                });
            }
            let hi = decode_hex(bytes[idx + 1])?;
            let lo = decode_hex(bytes[idx + 2])?;
            out.push((hi << 4) | lo);
            idx += 3;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }
    Ok(out)
}

fn decode_hex(value: u8) -> Result<u8, FetchError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(FetchError {
            message: "Invalid Gopher URL: invalid percent-encoding".to_string(),
            retryable: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{GopherItemType, GopherRequest, fetch_gopher_bytes, parse_gopher_url};
    use crate::config::SyncConfig;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::thread;
    use std::time::Duration;

    fn test_sync_config() -> SyncConfig {
        SyncConfig {
            parallel: 1,
            timeout_secs: 1,
            max_feed_bytes: 2 * 1024 * 1024,
            user_agent: "picofeedr-test".to_string(),
            retry_count: 0,
            retry_delay_secs: 0,
        }
    }

    #[test]
    fn parse_gopher_url_uses_default_port_and_empty_selector_for_root() {
        let request = parse_gopher_url("gopher://example.com").expect("request");
        assert_eq!(
            request,
            GopherRequest {
                host: "example.com".to_string(),
                port: 70,
                item_type: GopherItemType::Directory,
                selector: Vec::new(),
            }
        );
    }

    #[test]
    fn parse_gopher_url_supports_custom_port_and_percent_decoded_selector() {
        let request = parse_gopher_url("gopher://example.com:7070/0feed%2Exml").expect("request");
        assert_eq!(request.port, 7070);
        assert_eq!(request.item_type, GopherItemType::Text);
        assert_eq!(request.selector, b"feed.xml");
    }

    #[test]
    fn parse_gopher_url_rejects_search_form() {
        let error = parse_gopher_url("gopher://example.com/7selector%09query").expect_err("error");
        assert!(!error.retryable);
        assert!(
            error
                .message
                .contains("search and Gopher+ are not supported")
        );
    }

    #[test]
    fn parse_gopher_url_rejects_gopher_plus_form() {
        let error =
            parse_gopher_url("gopher://example.com/0selector%09query%09+").expect_err("error");
        assert!(!error.retryable);
        assert!(
            error
                .message
                .contains("search and Gopher+ are not supported")
        );
    }

    #[test]
    fn parse_gopher_url_rejects_unsupported_protocol_item_types() {
        let error = parse_gopher_url("gopher://example.com/8session").expect_err("error");
        assert!(!error.retryable);
        assert_eq!(error.message, "Unsupported Gopher item type");
    }

    fn wait_for_server(done_rx: Receiver<()>, label: &str) {
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|error| panic!("{label} did not complete: {error:?}"));
    }

    fn spawn_gopher_server<F>(handler: F) -> (String, Receiver<()>)
    where
        F: FnOnce(std::net::TcpStream) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let (done_tx, done_rx) = mpsc::channel();
        let _server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            handler(stream);
            done_tx.send(()).expect("Gopher server completion");
        });
        (format!("gopher://{addr}/0feed.xml"), done_rx)
    }

    fn read_request_line(stream: &mut std::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buf = [0_u8; 64];
        loop {
            let read = stream.read(&mut buf).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
            if request.ends_with(b"\r\n") {
                break;
            }
        }
        request
    }

    #[test]
    fn fetch_gopher_bytes_sends_selector_with_crlf_and_unescapes_dot_stuffed_lines() {
        let (url, server) = spawn_gopher_server(|mut stream| {
            let request = read_request_line(&mut stream);
            assert_eq!(request, b"feed.xml\r\n");
            stream
                .write_all(b"<rss>\r\n..dot\r\n</rss>\r\n.\r\n")
                .expect("write response");
        });

        let bytes = fetch_gopher_bytes(&url, &test_sync_config()).expect("fetch");
        wait_for_server(server, "Gopher server");

        assert_eq!(bytes, b"<rss>\r\n.dot\r\n</rss>\r\n");
    }

    #[test]
    fn fetch_gopher_bytes_reads_binary_response_until_eof() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let (done_tx, done_rx) = mpsc::channel();
        let _server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let request = read_request_line(&mut stream);
            assert_eq!(request, b"blob.bin\r\n");
            stream.write_all(b"\x00\x01.\r\n\xff").expect("write body");
            done_tx.send(()).expect("Gopher binary server completion");
        });

        let url = format!("gopher://{addr}/9blob.bin");
        let bytes = fetch_gopher_bytes(&url, &test_sync_config()).expect("fetch");
        wait_for_server(done_rx, "Gopher binary server");

        assert_eq!(bytes, b"\x00\x01.\r\n\xff");
    }

    #[test]
    fn fetch_gopher_bytes_retries_transient_read_failure_then_succeeds() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let (done_tx, done_rx) = mpsc::channel();
        let _server = thread::spawn(move || {
            let (mut first, _) = listener.accept().expect("accept first connection");
            let request = read_request_line(&mut first);
            assert_eq!(request, b"feed.xml\r\n");

            let (mut second, _) = listener.accept().expect("accept retry connection");
            let request = read_request_line(&mut second);
            assert_eq!(request, b"feed.xml\r\n");
            second
                .write_all(b"<rss></rss>\r\n.\r\n")
                .expect("write retry response");
            done_tx.send(()).expect("Gopher retry server completion");
        });

        let mut sync = test_sync_config();
        sync.retry_count = 1;
        let url = format!("gopher://{addr}/0feed.xml");

        let bytes = fetch_gopher_bytes(&url, &sync).expect("fetch after retry");
        wait_for_server(done_rx, "Gopher retry server");

        assert_eq!(bytes, b"<rss></rss>\r\n");
    }

    #[test]
    fn fetch_gopher_bytes_rejects_oversized_body() {
        let (url, server) = spawn_gopher_server(|mut stream| {
            let _ = read_request_line(&mut stream);
            stream
                .write_all(b"<rss>123456789</rss>\r\n.\r\n")
                .expect("write response");
        });
        let mut sync = test_sync_config();
        sync.max_feed_bytes = 8;

        let error = fetch_gopher_bytes(&url, &sync).expect_err("error");
        wait_for_server(server, "Gopher server");

        assert!(!error.retryable);
        assert_eq!(error.message, "Feed body exceeds max_feed_bytes");
    }

    #[test]
    fn fetch_gopher_bytes_connection_failure_is_retryable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        drop(listener);

        let error = fetch_gopher_bytes(&format!("gopher://{addr}/0feed.xml"), &test_sync_config())
            .expect_err("error");

        assert!(error.retryable);
        assert!(error.message.contains("Failed to connect to Gopher server"));
    }
}
