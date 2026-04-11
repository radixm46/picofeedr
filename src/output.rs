//! Output rendering utilities for CLI responses.

use crate::RunFailure;
use picofeedr::config::feeds::ConfigCheckReport;
use picofeedr::config::feeds::ValidationIssue;
use picofeedr::entry::{EntryDetail, EntryListResponse};
use picofeedr::feed::FeedListResponse;
use picofeedr::response::{MarkResponse, ResponsePayload, VersionResponse};
use picofeedr::status::StatusResponse;
use picofeedr::sync;
use picofeedr::sync::SyncSummary;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Write};
use time::{OffsetDateTime, UtcOffset};

/// Plain CLI output payloads.
pub(crate) enum PlainOutput {
    Ping,
    Version(VersionResponse),
    Tags(Vec<String>),
    Status(StatusResponse),
    Feeds {
        feeds: FeedListResponse,
        include_id: bool,
    },
    Sync(SyncSummary),
    List {
        list: EntryListResponse,
        include_id: bool,
    },
    View(EntryDetail),
    Mark(MarkResponse),
}

struct PlainTextOutput {
    stdout: String,
    stderr: String,
}

/// Writes a JSON response payload.
pub(crate) fn write_json_response<T: ResponsePayload>(payload: T) -> Result<(), RunFailure> {
    print_json_or_fallback(&payload.into_envelope())?;
    Ok(())
}

/// Writes human-readable output for a command result.
pub(crate) fn write_plain_output(result: PlainOutput) -> io::Result<()> {
    let rendered = format_plain_output(result);
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    writer.write_all(rendered.stdout.as_bytes())?;
    writer.flush()?;
    if !rendered.stderr.is_empty() {
        let stderr = io::stderr();
        let mut err_writer = io::BufWriter::new(stderr.lock());
        err_writer.write_all(rendered.stderr.as_bytes())?;
        err_writer.flush()?;
    }
    Ok(())
}

fn format_plain_output(result: PlainOutput) -> PlainTextOutput {
    let mut stdout = String::new();
    let mut stderr = String::new();
    match result {
        PlainOutput::Ping => {
            writeln!(stdout, "status: ok").expect("write ping");
        }
        PlainOutput::Version(payload) => {
            writeln!(stdout, "api_version: {}", payload.api_version).expect("write version api");
            writeln!(stdout, "db_schema_version: {}", payload.db_schema_version)
                .expect("write version schema");
            writeln!(stdout, "build: {}", payload.build).expect("write version build");
        }
        PlainOutput::Tags(tags) => {
            for tag in tags {
                writeln!(stdout, "{tag}").expect("write tag");
            }
        }
        PlainOutput::Status(status) => {
            writeln!(stdout, "revision: {}", status.revision).expect("write status revision");
            writeln!(
                stdout,
                "last_write_at: {}",
                format_plain_timestamp(status.last_write_at)
            )
            .expect("write status last_write_at");
            writeln!(stdout, "db_schema_version: {}", status.db_schema_version)
                .expect("write status schema");
            writeln!(stdout, "api_version: {}", status.api_version).expect("write status api");
            writeln!(
                stdout,
                "last_sync_at: {}",
                format_plain_timestamp(status.last_sync_at)
            )
            .expect("write status last_sync_at");
            writeln!(
                stdout,
                "last_sync_status: {}",
                status.last_sync_status.as_deref().unwrap_or("null")
            )
            .expect("write status sync_status");
        }
        PlainOutput::Feeds { feeds, include_id } => {
            for feed in feeds.feeds {
                let title = feed.title.as_deref().unwrap_or("");
                let site_url = feed.site_url.as_deref().unwrap_or("");
                let author = feed.author.as_deref().unwrap_or("");
                let tags = format_tags(&feed.tags);
                if include_id {
                    writeln!(
                        stdout,
                        "{title}\t{}\t{site_url}\t{author}\t{tags}\t{}",
                        feed.url, feed.feed_id
                    )
                    .expect("write feed row with id");
                } else {
                    writeln!(
                        stdout,
                        "{title}\t{}\t{site_url}\t{author}\t{tags}",
                        feed.url
                    )
                    .expect("write feed row");
                }
            }
        }
        PlainOutput::Sync(summary) => {
            writeln!(stdout, "status: {}", summary.status.as_str()).expect("write sync status");
            writeln!(stdout, "fetched_feed_count: {}", summary.fetched_feed_count)
                .expect("write sync fetched count");
            writeln!(stdout, "failed_feed_count: {}", summary.failed_feed_count)
                .expect("write sync failed count");
            writeln!(stdout, "new_entry_count: {}", summary.new_entry_count)
                .expect("write sync new count");
            writeln!(stdout, "duration_ms: {}", summary.duration_ms).expect("write sync duration");
            writeln!(stdout, "errors: {}", summary.errors.len()).expect("write sync error count");
            for (index, error) in summary.errors.into_iter().enumerate() {
                writeln!(stdout, "errors[{index}].feed_id: {}", error.feed_id)
                    .expect("write sync error feed_id");
                if let Some(feed_name) = error.feed_name.as_deref() {
                    writeln!(stdout, "errors[{index}].feed_name: {feed_name}")
                        .expect("write sync error feed_name");
                }
                writeln!(stdout, "errors[{index}].feed_url: {}", error.feed_url)
                    .expect("write sync error feed_url");
                writeln!(stdout, "errors[{index}].code: {}", error.code.as_str())
                    .expect("write sync error code");
                writeln!(stdout, "errors[{index}].retryable: {}", error.retryable)
                    .expect("write sync error retryable");
                writeln!(stdout, "errors[{index}].message: {}", error.message)
                    .expect("write sync error message");
            }
        }
        PlainOutput::List { list, include_id } => {
            let total_count = list.total_count;
            let next_page_token = list.next_page_token;
            let feed_titles = list
                .feeds
                .into_iter()
                .map(|feed| (feed.feed_id, feed.title.unwrap_or_default()))
                .collect::<HashMap<_, _>>();
            for entry in list.items {
                let date = format_plain_epoch(entry.published_at.unwrap_or(entry.first_seen_at));
                let title = entry.title.as_deref().unwrap_or("");
                let feed_title = feed_titles
                    .get(&entry.feed_id)
                    .map(String::as_str)
                    .unwrap_or("");
                let tags = format_tags(&entry.tags);
                let link = entry.link.as_deref().unwrap_or("");
                if include_id {
                    writeln!(
                        stdout,
                        "{date}\t{title}\t{feed_title}\t{tags}\t{link}\t{}",
                        entry.entry_id
                    )
                    .expect("write list line with id");
                } else {
                    writeln!(stdout, "{date}\t{title}\t{feed_title}\t{tags}\t{link}")
                        .expect("write list line");
                }
            }
            writeln!(stderr, "total_count: {total_count}").expect("write list total_count");
            if let Some(cursor) = next_page_token {
                writeln!(stderr, "next_page_token: {cursor}").expect("write list cursor");
            }
        }
        PlainOutput::View(detail) => {
            writeln!(stdout, "entry_id: {}", detail.entry_id).expect("write view entry_id");
            writeln!(
                stdout,
                "title: {}",
                format_plain_optional_str(detail.title.as_deref())
            )
            .expect("write view title");
            writeln!(stdout, "feed_id: {}", detail.feed_id).expect("write view feed_id");
            writeln!(
                stdout,
                "feed_title: {}",
                format_plain_optional_str(detail.feed_title.as_deref())
            )
            .expect("write view feed_title");
            writeln!(
                stdout,
                "author: {}",
                format_plain_optional_str(detail.author.as_deref())
            )
            .expect("write view author");
            writeln!(
                stdout,
                "link: {}",
                format_plain_optional_str(detail.link.as_deref())
            )
            .expect("write view link");
            writeln!(
                stdout,
                "published_at: {}",
                format_plain_timestamp(detail.published_at)
            )
            .expect("write view published");
            writeln!(
                stdout,
                "first_seen_at: {}",
                format_plain_epoch(detail.first_seen_at)
            )
            .expect("write view first_seen");
            if !detail.tags.is_empty() {
                writeln!(stdout, "tags: {}", format_tags(&detail.tags)).expect("write view tags");
            }
            if let Some(content) = &detail.content {
                writeln!(stdout).expect("write view spacer");
                writeln!(stdout, "{content}").expect("write view content");
            }
        }
        PlainOutput::Mark(payload) => {
            writeln!(
                stdout,
                "updated_entry_count: {}",
                payload.updated_entry_count
            )
            .expect("write mark");
        }
    }
    PlainTextOutput { stdout, stderr }
}

/// Formats epoch seconds for plain output as local datetime.
fn format_plain_timestamp(value: Option<i64>) -> String {
    value
        .map(format_plain_epoch)
        .unwrap_or_else(|| "null".to_string())
}

fn format_plain_optional_str(value: Option<&str>) -> String {
    value.unwrap_or("null").to_string()
}

/// Formats one epoch timestamp using local offset (fallback: UTC).
fn format_plain_epoch(epoch: i64) -> String {
    let local = epoch_to_local(epoch);
    let offset = local.offset();
    let month = u8::from(local.month());
    let offset_hours = offset.whole_hours();
    let offset_minutes = offset.minutes_past_hour().abs();

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{:+03}:{:02}",
        local.year(),
        month,
        local.day(),
        local.hour(),
        local.minute(),
        local.second(),
        offset_hours,
        offset_minutes
    )
}

/// Converts epoch seconds to local datetime (fallback: UTC).
fn epoch_to_local(epoch: i64) -> OffsetDateTime {
    let utc = OffsetDateTime::from_unix_timestamp(epoch).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    utc.to_offset(offset)
}

/// Writes one sync progress event as a plain output line.
pub(crate) fn write_sync_progress_line<W: Write>(
    writer: &mut W,
    event: &sync::SyncProgressEvent,
) -> io::Result<()> {
    writeln!(writer, "{}", format_sync_progress_line(event))
}

fn format_sync_progress_line(event: &sync::SyncProgressEvent) -> String {
    match event {
        sync::SyncProgressEvent::Start { total_feeds } => {
            format!("sync:start total_feeds={total_feeds}")
        }
        sync::SyncProgressEvent::FeedStart {
            index,
            total_feeds,
            url,
        } => format!("sync:feed start index={index}/{total_feeds} url={url}"),
        sync::SyncProgressEvent::FeedOk {
            index,
            total_feeds,
            url,
            entries,
        } => format!("sync:feed ok index={index}/{total_feeds} url={url} entries={entries}"),
        sync::SyncProgressEvent::FeedError {
            index,
            total_feeds,
            url,
            code,
            retryable,
        } => format!(
            "sync:feed error index={index}/{total_feeds} url={url} code={} retryable={retryable}",
            code.as_str()
        ),
    }
}

/// Writes human-readable output for feeds config validation.
pub(crate) fn write_config_check_plain(report: &ConfigCheckReport) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    writer.write_all(format_config_check_plain(report).as_bytes())?;
    writer.flush()?;
    Ok(())
}

fn format_config_check_plain(report: &ConfigCheckReport) -> String {
    let mut output = String::new();
    writeln!(output, "valid: {}", report.valid).expect("write config-check valid");
    writeln!(output, "checked_feeds: {}", report.checked_feeds)
        .expect("write config-check checked_feeds");
    writeln!(output, "errors: {}", report.errors.len()).expect("write config-check errors");
    writeln!(output, "warnings: {}", report.warnings.len()).expect("write config-check warnings");
    write_validation_issue_lines(&mut output, "error", &report.errors);
    write_validation_issue_lines(&mut output, "warning", &report.warnings);
    output
}

fn write_validation_issue_lines(output: &mut String, kind: &str, issues: &[ValidationIssue]) {
    for issue in issues {
        let mut line = format!("{kind}: code={}", issue.code);
        if let Some(path) = issue.path.as_deref() {
            write!(line, " path={path}").expect("write issue path");
        }
        writeln!(output, "{line}").expect("write issue line");
        writeln!(output, "  message: {}", issue.message).expect("write issue message");
    }
}

/// Prints JSON to stdout, falling back to a hard-coded INTERNAL error JSON on failure.
pub(crate) fn print_json_or_fallback<T: serde::Serialize>(value: &T) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    match serde_json::to_string(value) {
        Ok(json) => writeln!(writer, "{json}")?,
        Err(_) => writeln!(writer, "{FALLBACK_INTERNAL_ERROR_JSON}")?,
    }
    writer.flush()
}

/// Formats tags for plain output.
fn format_tags(tags: &[String]) -> String {
    tags.join(", ")
}

/// Fallback JSON printed when JSON serialization fails unexpectedly.
const FALLBACK_INTERNAL_ERROR_JSON: &str = "{\"status\":\"error\",\"result\":null,\"error\":{\"code\":\"INTERNAL\",\"message\":\"Failed to serialize response\",\"retryable\":false,\"details\":null},\"meta\":{\"api_version\":\"unknown\",\"db_schema_version\":0,\"generated_at\":0}}";

#[cfg(test)]
mod tests {
    use super::{PlainOutput, format_plain_output, format_sync_progress_line, format_tags};
    use picofeedr::entry::{EntryListResponse, EntrySummary, FeedSummary};
    use picofeedr::sync::SyncProgressEvent;

    #[test]
    fn format_tags_joins_with_comma_and_space() {
        assert_eq!(format_tags(&[]), "");
        assert_eq!(
            format_tags(&["a".to_string(), "b".to_string(), "c".to_string()]),
            "a, b, c"
        );
    }

    #[test]
    fn format_sync_progress_line_renders_public_event_variants() {
        let start = format_sync_progress_line(&SyncProgressEvent::Start { total_feeds: 2 });
        let feed_start = format_sync_progress_line(&SyncProgressEvent::FeedStart {
            index: 1,
            total_feeds: 2,
            url: "https://example.com/feed.xml".to_string(),
        });
        let feed_ok = format_sync_progress_line(&SyncProgressEvent::FeedOk {
            index: 1,
            total_feeds: 2,
            url: "https://example.com/feed.xml".to_string(),
            entries: 3,
        });

        assert_eq!(start, "sync:start total_feeds=2");
        assert_eq!(
            feed_start,
            "sync:feed start index=1/2 url=https://example.com/feed.xml"
        );
        assert_eq!(
            feed_ok,
            "sync:feed ok index=1/2 url=https://example.com/feed.xml entries=3"
        );
    }

    #[test]
    fn format_plain_output_routes_list_metadata_to_stderr() {
        let rendered = format_plain_output(PlainOutput::List {
            list: EntryListResponse {
                total_count: 42,
                items: vec![EntrySummary {
                    entry_id: "entry-1".to_string(),
                    feed_id: "feed-1".to_string(),
                    title: Some("Hello".to_string()),
                    link: Some("https://example.com/1".to_string()),
                    published_at: Some(1_704_067_200),
                    first_seen_at: 1_704_067_200,
                    tags: vec!["rust".to_string()],
                }],
                feeds: vec![FeedSummary {
                    feed_id: "feed-1".to_string(),
                    title: Some("Feed".to_string()),
                }],
                next_page_token: Some("cursor-1".to_string()),
                revision: 7,
                last_write_at: Some(1_704_067_200),
            },
            include_id: true,
        });

        assert!(rendered.stdout.contains("Hello"));
        assert!(rendered.stdout.contains("entry-1"));
        assert!(rendered.stderr.contains("total_count: 42"));
        assert!(rendered.stderr.contains("next_page_token: cursor-1"));
    }
}
