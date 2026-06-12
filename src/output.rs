//! Output rendering utilities for CLI responses.

use crate::{CommandOutcome, RunFailure};
use picofeedr::cli::OutputFormat;
use picofeedr::config::feeds::ConfigCheckReport;
use picofeedr::config::feeds::ValidationIssue;
use picofeedr::response::ResponsePayload;
use picofeedr::sync;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Write};
use time::{OffsetDateTime, UtcOffset};

struct PlainTextOutput {
    stdout: String,
    stderr: String,
}

/// Writes a command outcome in the selected output format.
pub(crate) fn write_command_output(
    output: OutputFormat,
    outcome: CommandOutcome,
) -> Result<(), RunFailure> {
    match output {
        OutputFormat::Json => write_json_output(outcome),
        OutputFormat::Plain => {
            write_plain_output(outcome)?;
            Ok(())
        }
    }
}

/// Writes the JSON envelope for a command outcome.
fn write_json_output(outcome: CommandOutcome) -> Result<(), RunFailure> {
    match outcome {
        CommandOutcome::Version(payload) => write_json_response(payload),
        CommandOutcome::Tags(payload) => write_json_response(payload),
        CommandOutcome::Status(payload) => write_json_response(payload),
        CommandOutcome::Feeds { feeds, .. } => write_json_response(feeds),
        CommandOutcome::Sync(payload) => write_json_response(payload),
        CommandOutcome::SyncCheck(payload) => write_json_response(payload),
        CommandOutcome::List { list, .. } => write_json_response(list),
        CommandOutcome::View(payload) => write_json_response(payload),
        CommandOutcome::Mark(payload) => write_json_response(payload),
    }
}

/// Writes a JSON response payload.
fn write_json_response<T: ResponsePayload>(payload: T) -> Result<(), RunFailure> {
    print_json_or_fallback(&payload.into_envelope())?;
    Ok(())
}

/// Writes human-readable output for a command result.
fn write_plain_output(result: CommandOutcome) -> io::Result<()> {
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

fn format_plain_output(result: CommandOutcome) -> PlainTextOutput {
    let mut stdout = String::new();
    let mut stderr = String::new();
    match result {
        CommandOutcome::Version(payload) => {
            writeln!(stdout, "api_version: {}", payload.api_version).expect("write version api");
            writeln!(stdout, "db_schema_version: {}", payload.db_schema_version)
                .expect("write version schema");
            writeln!(stdout, "build: {}", payload.build).expect("write version build");
        }
        CommandOutcome::Tags(payload) => {
            for tag in payload.tags {
                writeln!(stdout, "{tag}").expect("write tag");
            }
        }
        CommandOutcome::Status(status) => {
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
        CommandOutcome::Feeds { feeds, include_id } => {
            for feed in feeds.feeds {
                let title = feed.title.as_deref().unwrap_or("");
                let site_url = feed.site_url.as_deref().unwrap_or("");
                let author = feed.author.as_deref().unwrap_or("");
                if include_id {
                    writeln!(
                        stdout,
                        "{title}\t{}\t{site_url}\t{author}\t{}",
                        feed.url, feed.feed_id
                    )
                    .expect("write feed row with id");
                } else {
                    writeln!(stdout, "{title}\t{}\t{site_url}\t{author}", feed.url)
                        .expect("write feed row");
                }
            }
        }
        CommandOutcome::Sync(summary) => {
            writeln!(
                stdout,
                "sync:done status={} fetched_feed_count={} skipped_feed_count={} failed_feed_count={} new_entry_count={} duration_ms={} errors={}",
                summary.status.as_str(),
                summary.fetched_feed_count,
                summary.skipped_feed_count,
                summary.failed_feed_count,
                summary.new_entry_count,
                summary.duration_ms,
                summary.errors.len()
            )
            .expect("write sync done");
            for error in summary.errors {
                let mut line = format!(
                    "sync:feed-error url={} code={} retryable={}",
                    error.feed_url,
                    error.code.as_str(),
                    error.retryable
                );
                if let Some(feed_name) = error.feed_name.as_deref() {
                    write!(line, " feed_name={}", format_log_value(feed_name))
                        .expect("write sync error feed_name");
                }
                write!(line, " message={}", format_log_value(&error.message))
                    .expect("write sync error message");
                writeln!(stderr, "{line}").expect("write sync error line");
            }
        }
        CommandOutcome::SyncCheck(report) => {
            stdout.push_str(&format_config_check_plain(&report));
        }
        CommandOutcome::List { list, include_id } => {
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
        CommandOutcome::View(detail) => {
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
        CommandOutcome::Mark(payload) => {
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

fn format_log_value(value: &str) -> String {
    serde_json::to_string(value).expect("serialize log value")
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
    let Some(line) = format_sync_progress_line(event) else {
        return Ok(());
    };
    writeln!(writer, "{line}")
}

fn format_sync_progress_line(event: &sync::SyncProgressEvent) -> Option<String> {
    match event {
        sync::SyncProgressEvent::Start {
            total_feeds,
            skipped_feed_count,
        } => Some(format!(
            "sync:start total_feeds={total_feeds} skipped_feeds={skipped_feed_count}"
        )),
        sync::SyncProgressEvent::FeedSkip { url, feed_name } => {
            let mut line = format!("sync:skip url={url}");
            if let Some(feed_name) = feed_name.as_deref() {
                write!(line, " feed_name={}", format_log_value(feed_name))
                    .expect("write skipped feed_name");
            }
            Some(line)
        }
        sync::SyncProgressEvent::FeedStart { .. } => None,
        sync::SyncProgressEvent::FeedOk {
            index,
            total_feeds,
            url,
            entries,
        } => Some(format!(
            "sync:feed-ok index={index}/{total_feeds} url={url} entries={entries}"
        )),
        sync::SyncProgressEvent::FeedError {
            url: _,
            code: _,
            retryable: _,
        } => None,
    }
}

fn format_config_check_plain(report: &ConfigCheckReport) -> String {
    let mut output = String::new();
    writeln!(output, "valid: {}", report.valid).expect("write sync-check valid");
    writeln!(output, "checked_feeds: {}", report.checked_feeds)
        .expect("write sync-check checked_feeds");
    writeln!(output, "skipped_feeds: {}", report.skipped_feeds)
        .expect("write sync-check skipped_feeds");
    writeln!(output, "errors: {}", report.errors.len()).expect("write sync-check errors");
    writeln!(output, "warnings: {}", report.warnings.len()).expect("write sync-check warnings");
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
    use super::{format_plain_output, format_sync_progress_line, format_tags};
    use crate::CommandOutcome;
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
        let start = format_sync_progress_line(&SyncProgressEvent::Start {
            total_feeds: 2,
            skipped_feed_count: 1,
        });
        let feed_skip = format_sync_progress_line(&SyncProgressEvent::FeedSkip {
            url: "https://example.com/skipped.xml".to_string(),
            feed_name: Some("Skipped".to_string()),
        });
        let feed_start = format_sync_progress_line(&SyncProgressEvent::FeedStart {
            url: "https://example.com/feed.xml".to_string(),
        });
        let feed_ok = format_sync_progress_line(&SyncProgressEvent::FeedOk {
            index: 1,
            total_feeds: 2,
            url: "https://example.com/feed.xml".to_string(),
            entries: 3,
        });
        assert_eq!(
            start.as_deref(),
            Some("sync:start total_feeds=2 skipped_feeds=1")
        );
        assert_eq!(
            feed_skip.as_deref(),
            Some("sync:skip url=https://example.com/skipped.xml feed_name=\"Skipped\"")
        );
        assert!(feed_start.is_none());
        assert_eq!(
            feed_ok.as_deref(),
            Some("sync:feed-ok index=1/2 url=https://example.com/feed.xml entries=3")
        );
    }

    #[test]
    fn format_plain_output_routes_list_metadata_to_stderr() {
        let rendered = format_plain_output(CommandOutcome::List {
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
