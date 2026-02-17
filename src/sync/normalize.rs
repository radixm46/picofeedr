//! Entry normalization pipeline.

use crate::config::AppConfig;
use crate::error::AppError;
use crate::identity::EntryIdentity;
use crate::time::current_epoch;
use std::collections::HashSet;

use super::autotag::{CompiledRule, match_auto_tags};
use super::content::{build_entry_content, select_content};
use super::model::{PendingEntry, SyncEntry, SyncTarget};

/// Normalizes a feed entry into database payloads.
pub(crate) fn normalize_entry(
    entry: &feed_rs::model::Entry,
    target: &SyncTarget,
    rules: &[CompiledRule],
    config: &AppConfig,
) -> Result<SyncEntry, AppError> {
    let link = entry.links.first().map(|link| link.href.clone());
    let title = entry.title.as_ref().map(|title| title.content.clone());
    let author = entry.authors.first().map(|author| author.name.clone());
    let published_at = entry.published.map(|value| value.timestamp());
    let updated_at = entry.updated.map(|value| value.timestamp());
    let first_seen_at = current_epoch();

    let (content, content_type) = select_content(entry);
    let identity = EntryIdentity::from_entry(&target.feed_id, entry, content.as_deref());
    let entry_id = identity.entry_id;
    let content_plan = build_entry_content(config, content, content_type)?;

    let mut tags = Vec::new();
    tags.extend(target.tags.iter().cloned());
    let title_value = title.clone().unwrap_or_default();
    tags.extend(match_auto_tags(&title_value, rules));
    tags.push(config.unread_tag.clone());
    let tags = dedupe_tags(tags);

    Ok(SyncEntry {
        feed_id: target.feed_id.clone(),
        entry: PendingEntry {
            entry_id,
            source_id: Some(identity.source_id),
            link,
            title,
            author,
            published_at,
            updated_at,
            first_seen_at,
            meta_json: None,
        },
        content: content_plan.content,
        content_payload: content_plan.payload,
        tags,
    })
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
