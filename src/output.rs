//! Output rendering utilities for CLI responses.

use crate::{CommandOutput, RunFailure};
use picofeedr::config::feeds::ConfigCheckReport;
use picofeedr::response::ResponsePayload;
use picofeedr::sync;
use std::collections::HashMap;
use std::io::{self, Write};
use time::{OffsetDateTime, UtcOffset};

/// Writes JSON output for a command result.
pub(crate) fn write_json_output(result: CommandOutput) -> Result<(), RunFailure> {
    fn print_payload<T: ResponsePayload>(data: T) -> Result<(), RunFailure> {
        print_json_or_fallback(&data.into_envelope())?;
        Ok(())
    }

    match result {
        CommandOutput::Ping(payload) => print_payload(payload),
        CommandOutput::Version(payload) => print_payload(payload),
        CommandOutput::Tags(payload) => print_payload(payload),
        CommandOutput::Status(payload) => print_payload(payload),
        CommandOutput::FeedsList(payload) => print_payload(payload),
        CommandOutput::Sync(payload) => print_payload(payload),
        CommandOutput::List { list, .. } => print_payload(list),
        CommandOutput::View(detail) => print_payload(detail),
        CommandOutput::Mark(payload) => print_payload(payload),
    }
}

/// Writes human-readable output for a command result.
pub(crate) fn write_plain_output(result: CommandOutput) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    match result {
        CommandOutput::Ping(_) => writeln!(writer, "ok")?,
        CommandOutput::Version(payload) => writeln!(
            writer,
            "api_version={} db_schema_version={} build={}",
            payload.api_version, payload.db_schema_version, payload.build
        )?,
        CommandOutput::Tags(payload) => {
            for tag in payload.tags {
                writeln!(writer, "{tag}")?;
            }
        }
        CommandOutput::Status(status) => {
            writeln!(writer, "revision: {}", status.revision)?;
            writeln!(
                writer,
                "last_write_at: {}",
                format_plain_timestamp(status.last_write_at)
            )?;
            writeln!(writer, "db_schema_version: {}", status.db_schema_version)?;
            writeln!(writer, "api_version: {}", status.api_version)?;
            writeln!(
                writer,
                "last_sync_at: {}",
                format_plain_timestamp(status.last_sync_at)
            )?;
            writeln!(
                writer,
                "last_sync_status: {}",
                status.last_sync_status.as_deref().unwrap_or("null")
            )?;
        }
        CommandOutput::FeedsList(feeds) => {
            for feed in feeds.feeds {
                let title = feed.title.as_deref().unwrap_or("(untitled)");
                let tags = format_tags(&feed.tags);
                if tags.is_empty() {
                    writeln!(writer, "[{}] {}", feed.feed_id, title)?;
                } else {
                    writeln!(writer, "[{}] {} [{}]", feed.feed_id, title, tags)?;
                }
                writeln!(writer, "  url: {}", feed.url)?;
                if let Some(site_url) = &feed.site_url {
                    writeln!(writer, "  site: {site_url}")?;
                }
                if let Some(author) = &feed.author {
                    writeln!(writer, "  author: {author}")?;
                }
            }
        }
        CommandOutput::Sync(summary) => {
            writeln!(writer, "status: {}", summary.status.as_str())?;
            writeln!(
                writer,
                "fetched_feed_count: {} failed_feed_count: {} new_entry_count: {} duration_ms: {}",
                summary.fetched_feed_count,
                summary.failed_feed_count,
                summary.new_entry_count,
                summary.duration_ms
            )?;
            if !summary.errors.is_empty() {
                writeln!(writer, "errors: {}", summary.errors.len())?;
                for error in summary.errors {
                    let feed_name = error.feed_name.as_deref().unwrap_or("(untitled)");
                    writeln!(
                        writer,
                        "  {} {} {} retryable={}",
                        feed_name,
                        error.feed_url,
                        error.code.as_str(),
                        error.retryable
                    )?;
                    writeln!(writer, "    {}", error.message)?;
                }
            }
        }
        CommandOutput::List { list, include_id } => {
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
                        writer,
                        "{date}\t{title}\t{feed_title}\t{tags}\t{link}\t{}",
                        entry.entry_id
                    )?;
                } else {
                    writeln!(writer, "{date}\t{title}\t{feed_title}\t{tags}\t{link}")?;
                }
            }
            writer.flush()?;
            let stderr = io::stderr();
            let mut err_writer = io::BufWriter::new(stderr.lock());
            writeln!(err_writer, "total_count: {total_count}")?;
            if let Some(cursor) = next_page_token {
                writeln!(err_writer, "next_page_token: {cursor}")?;
            }
            err_writer.flush()?;
        }
        CommandOutput::View(detail) => {
            let title = detail.title.as_deref().unwrap_or("(untitled)");
            writeln!(writer, "{} {title}", detail.entry_id)?;
            if let Some(feed_title) = &detail.feed_title {
                writeln!(writer, "feed: {feed_title} (id: {})", detail.feed_id)?;
            } else {
                writeln!(writer, "feed_id: {}", detail.feed_id)?;
            }
            if let Some(author) = &detail.author {
                writeln!(writer, "author: {author}")?;
            }
            if let Some(link) = &detail.link {
                writeln!(writer, "link: {link}")?;
            }
            if !detail.tags.is_empty() {
                writeln!(writer, "tags: {}", format_tags(&detail.tags))?;
            }
            if let Some(published) = detail.published_at {
                writeln!(writer, "published_at: {published}")?;
            }
            writeln!(writer, "first_seen_at: {}", detail.first_seen_at)?;
            if let Some(content) = &detail.content {
                writeln!(writer)?;
                writeln!(writer, "{content}")?;
            }
        }
        CommandOutput::Mark(payload) => writeln!(
            writer,
            "updated_entry_count: {}",
            payload.updated_entry_count
        )?,
    }
    writer.flush()?;
    Ok(())
}

/// Formats epoch seconds for plain output as local datetime.
fn format_plain_timestamp(value: Option<i64>) -> String {
    value
        .map(format_plain_epoch)
        .unwrap_or_else(|| "null".to_string())
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
    match event {
        sync::SyncProgressEvent::Start { total_feeds } => {
            writeln!(writer, "sync:start total_feeds={total_feeds}")
        }
        sync::SyncProgressEvent::FeedStart {
            index,
            total_feeds,
            url,
        } => writeln!(
            writer,
            "sync:feed start index={index}/{total_feeds} url={url}"
        ),
        sync::SyncProgressEvent::FeedOk {
            index,
            total_feeds,
            url,
            entries,
        } => writeln!(
            writer,
            "sync:feed ok index={index}/{total_feeds} url={url} entries={entries}"
        ),
        sync::SyncProgressEvent::FeedError {
            index,
            total_feeds,
            url,
            code,
            retryable,
        } => writeln!(
            writer,
            "sync:feed error index={index}/{total_feeds} url={url} code={} retryable={retryable}",
            code.as_str()
        ),
    }
}

/// Writes human-readable output for feeds config validation.
pub(crate) fn write_config_check_plain(report: &ConfigCheckReport) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    writeln!(writer, "valid: {}", report.valid)?;
    writeln!(writer, "checked_feeds: {}", report.checked_feeds)?;
    writeln!(writer, "errors: {}", report.errors.len())?;
    for issue in &report.errors {
        if let Some(path) = &issue.path {
            writeln!(writer, "  {} {} ({path})", issue.code, issue.message)?;
        } else {
            writeln!(writer, "  {} {}", issue.code, issue.message)?;
        }
    }
    writeln!(writer, "warnings: {}", report.warnings.len())?;
    for issue in &report.warnings {
        if let Some(path) = &issue.path {
            writeln!(writer, "  {} {} ({path})", issue.code, issue.message)?;
        } else {
            writeln!(writer, "  {} {}", issue.code, issue.message)?;
        }
    }
    writer.flush()?;
    Ok(())
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
    use super::{format_tags, write_sync_progress_line};
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
    fn write_sync_progress_line_renders_public_event_variants() {
        let mut out = Vec::<u8>::new();
        write_sync_progress_line(&mut out, &SyncProgressEvent::Start { total_feeds: 2 })
            .expect("start line");
        write_sync_progress_line(
            &mut out,
            &SyncProgressEvent::FeedStart {
                index: 1,
                total_feeds: 2,
                url: "https://example.com/feed.xml".to_string(),
            },
        )
        .expect("feed start line");
        write_sync_progress_line(
            &mut out,
            &SyncProgressEvent::FeedOk {
                index: 1,
                total_feeds: 2,
                url: "https://example.com/feed.xml".to_string(),
                entries: 3,
            },
        )
        .expect("feed ok line");

        let s = String::from_utf8(out).expect("utf8 output");
        assert!(s.contains("sync:start total_feeds=2"));
        assert!(s.contains("sync:feed start index=1/2 url=https://example.com/feed.xml"));
        assert!(s.contains("sync:feed ok index=1/2 url=https://example.com/feed.xml entries=3"));
    }
}
