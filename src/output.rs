//! Output rendering utilities for CLI responses.

use crate::{CommandOutput, RunFailure};
use picofeedr::config::feeds::ConfigCheckReport;
use picofeedr::response::{
    Envelope, MarkResult, PingResult, ResponseStatus, TagsResult, VersionResult,
};
use picofeedr::sync;
use std::collections::HashMap;
use std::io::{self, Write};

/// Renders JSON output for a command result.
pub(crate) fn render_json(result: &CommandOutput) -> Result<(), RunFailure> {
    fn print_success<T: serde::Serialize>(
        data: T,
        status: ResponseStatus,
    ) -> Result<(), RunFailure> {
        print_json_or_fallback(&Envelope::ok_with_status(data, status))?;
        Ok(())
    }

    match result {
        CommandOutput::Ping => print_success(PingResult::ok(), ResponseStatus::Ok),
        CommandOutput::Version {
            api_version,
            db_schema_version,
            build,
        } => print_success(
            VersionResult {
                api_version: (*api_version).to_string(),
                db_schema_version: *db_schema_version,
                build: (*build).to_string(),
            },
            ResponseStatus::Ok,
        ),
        CommandOutput::Tags { tags } => {
            print_success(TagsResult { tags: tags.clone() }, ResponseStatus::Ok)
        }
        CommandOutput::Status { status } => print_success(status, ResponseStatus::Ok),
        CommandOutput::FeedsList { feeds } => print_success(feeds, ResponseStatus::Ok),
        CommandOutput::Sync { summary } => {
            let envelope_status = if matches!(summary.status, sync::SyncStatus::Completed) {
                ResponseStatus::Ok
            } else {
                ResponseStatus::Warning
            };
            print_success(summary, envelope_status)
        }
        CommandOutput::List { list } => print_success(list, ResponseStatus::Ok),
        CommandOutput::View { detail } => print_success(detail, ResponseStatus::Ok),
        CommandOutput::Mark { updated } => print_success(
            MarkResult {
                updated_entry_count: *updated,
            },
            ResponseStatus::Ok,
        ),
    }
}

/// Renders human-readable output for a command result.
pub(crate) fn render_plain(result: &CommandOutput) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    match result {
        CommandOutput::Ping => writeln!(writer, "ok")?,
        CommandOutput::Version {
            api_version,
            db_schema_version,
            build,
        } => writeln!(
            writer,
            "api_version={api_version} db_schema_version={db_schema_version} build={build}"
        )?,
        CommandOutput::Tags { tags } => {
            for tag in tags {
                writeln!(writer, "{tag}")?;
            }
        }
        CommandOutput::Status { status } => {
            writeln!(writer, "revision: {}", status.revision)?;
            writeln!(
                writer,
                "last_write_at: {}",
                status
                    .last_write_at
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_string())
            )?;
            writeln!(writer, "db_schema_version: {}", status.db_schema_version)?;
            writeln!(writer, "api_version: {}", status.api_version)?;
            writeln!(
                writer,
                "last_sync_at: {}",
                status
                    .last_sync_at
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_string())
            )?;
            writeln!(
                writer,
                "last_sync_status: {}",
                status.last_sync_status.as_deref().unwrap_or("null")
            )?;
        }
        CommandOutput::FeedsList { feeds } => {
            for feed in &feeds.feeds {
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
        CommandOutput::Sync { summary } => {
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
                for error in &summary.errors {
                    writeln!(
                        writer,
                        "  {} {} retryable={}",
                        error.feed_url,
                        error.code.as_str(),
                        error.retryable
                    )?;
                    writeln!(writer, "    {}", error.message)?;
                }
            }
        }
        CommandOutput::List { list } => {
            writeln!(writer, "total_count: {}", list.total_count)?;
            if let Some(cursor) = &list.next_page_token {
                writeln!(writer, "next_page_token: {cursor}")?;
            }
            let feed_titles = list
                .feeds
                .iter()
                .map(|feed| {
                    (
                        feed.feed_id.clone(),
                        feed.title.as_deref().unwrap_or("(untitled)").to_string(),
                    )
                })
                .collect::<HashMap<_, _>>();
            for entry in &list.items {
                let title = entry.title.as_deref().unwrap_or("(untitled)");
                let feed_title = feed_titles
                    .get(&entry.feed_id)
                    .map(String::as_str)
                    .unwrap_or("(unknown)");
                let tags = format_tags(&entry.tags);
                if tags.is_empty() {
                    writeln!(writer, "[{}] {title} ({feed_title})", entry.entry_id)?;
                } else {
                    writeln!(
                        writer,
                        "[{}] {title} ({feed_title}) [{tags}]",
                        entry.entry_id
                    )?;
                }
            }
        }
        CommandOutput::View { detail } => {
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
        CommandOutput::Mark { updated } => writeln!(writer, "updated_entry_count: {updated}")?,
    }
    writer.flush()?;
    Ok(())
}

/// Renders one sync progress event as a plain output line.
pub(crate) fn render_sync_progress_line<W: Write>(
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

/// Renders human-readable output for feeds config validation.
pub(crate) fn render_config_check_plain(report: &ConfigCheckReport) -> io::Result<()> {
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
    use super::{format_tags, render_sync_progress_line};
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
    fn render_sync_progress_line_renders_public_event_variants() {
        let mut out = Vec::<u8>::new();
        render_sync_progress_line(&mut out, &SyncProgressEvent::Start { total_feeds: 2 })
            .expect("start line");
        render_sync_progress_line(
            &mut out,
            &SyncProgressEvent::FeedStart {
                index: 1,
                total_feeds: 2,
                url: "https://example.com/feed.xml".to_string(),
            },
        )
        .expect("feed start line");
        render_sync_progress_line(
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
