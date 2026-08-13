//! Entry normalization pipeline.

use crate::config::AppConfig;
use crate::identity::entry_id_from_entry;
use crate::string_set::dedupe_strings_preserve_order;
use crate::time::current_epoch;

use super::autotag::match_auto_tags;
use super::content::{build_entry_content, select_content};
use super::model::{PendingEntry, SyncEntry, SyncTarget};

/// Normalizes a feed entry into database payloads.
pub(crate) fn normalize_entry(
    entry: &feed_rs::model::Entry,
    target: &SyncTarget,
    config: &AppConfig,
) -> SyncEntry {
    let link = entry.links.first().map(|link| link.href.clone());
    let title = entry.title.as_ref().map(|title| title.content.clone());
    let author = entry.authors.first().map(|author| author.name.clone());
    let published_at = entry.published.map(|value| value.timestamp());
    let updated_at = entry.updated.map(|value| value.timestamp());
    let first_seen_at = current_epoch();

    let (content, content_type) = select_content(entry);
    let entry_id = entry_id_from_entry(&target.ctx.feed_id, entry, content.as_deref());
    let content_plan = build_entry_content(config, content, content_type);

    let mut tags = Vec::new();
    tags.extend(target.tags.iter().cloned());
    tags.extend(match_auto_tags(
        title.as_deref().unwrap_or(""),
        &target.auto_tag_rules,
    ));
    if let Some(unread_tag) = config.auto_unread_tag() {
        tags.push(unread_tag.to_string());
    }
    let tags = dedupe_strings_preserve_order(tags);

    SyncEntry {
        entry: PendingEntry {
            entry_id,
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
    }
}
