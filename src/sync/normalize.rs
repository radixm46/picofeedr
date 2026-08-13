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

#[cfg(test)]
mod tests {
    use super::normalize_entry;
    use crate::config::feeds::AutoTagRule;
    use crate::config::{
        AppConfig, CliConfig, ContentStore, DatabaseConfig, FeedsSourceConfig, QueryConfig,
        StorageConfig, SyncConfig,
    };
    use crate::sync::autotag::compile_auto_tags;
    use crate::sync::model::{FeedContext, SyncTarget};
    use std::io::Cursor;
    use std::path::PathBuf;

    #[test]
    fn normalize_entry_deduplicates_colliding_target_auto_and_unread_tags() {
        let feed = feed_rs::parser::parse(Cursor::new(
            r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Test feed</title>
    <link>https://example.com</link>
    <description>Test feed</description>
    <item>
      <guid>entry-1</guid>
      <title>Steam Weekly</title>
      <link>https://example.com/entry-1</link>
    </item>
  </channel>
</rss>"#,
        ))
        .expect("parse feed");
        let target = SyncTarget {
            ctx: FeedContext {
                feed_id: "feed-1".to_string(),
                feed_name: None,
                url: "https://example.com/feed.xml".to_string(),
            },
            tags: vec!["shared".to_string()],
            auto_tag_rules: compile_auto_tags(&[AutoTagRule {
                title_regex: None,
                title_contains: Some(vec!["Steam".to_string()]),
                add_tags: vec!["shared".to_string()],
                priority: None,
            }])
            .expect("compile auto-tag rules"),
        };
        let config = AppConfig {
            manage_unread: true,
            unread_tag: "shared".to_string(),
            database: DatabaseConfig {
                path: PathBuf::from("/tmp/db.sqlite"),
            },
            feeds: FeedsSourceConfig {
                source: PathBuf::from("/tmp/feeds.yaml"),
            },
            sync: SyncConfig {
                parallel: 1,
                timeout_secs: 1,
                max_feed_bytes: 2 * 1024 * 1024,
                user_agent: "picofeedr-test".to_string(),
                retry_count: 0,
                retry_delay_secs: 0,
            },
            storage: StorageConfig {
                root_dir: PathBuf::from("/tmp"),
                content_store: ContentStore::None,
                data_dir: PathBuf::from("/tmp/data"),
            },
            query: QueryConfig {
                default_limit: 100,
                max_limit: 1000,
            },
            cli: CliConfig {
                output: crate::cli::OutputFormat::Plain,
            },
        };

        let mut target_only = target.clone();
        target_only.tags = vec!["target".to_string()];
        target_only.auto_tag_rules.clear();
        let mut unread_disabled = config.clone();
        unread_disabled.manage_unread = false;
        assert!(
            normalize_entry(&feed.entries[0], &target_only, &unread_disabled)
                .tags
                .iter()
                .any(|tag| tag.as_str() == "target")
        );

        let mut auto_only = target.clone();
        auto_only.tags.clear();
        assert!(
            normalize_entry(&feed.entries[0], &auto_only, &unread_disabled)
                .tags
                .iter()
                .any(|tag| tag.as_str() == "shared")
        );

        let mut unread_only = target.clone();
        unread_only.tags.clear();
        unread_only.auto_tag_rules.clear();
        assert!(
            normalize_entry(&feed.entries[0], &unread_only, &config)
                .tags
                .iter()
                .any(|tag| tag.as_str() == "shared")
        );

        let normalized = normalize_entry(&feed.entries[0], &target, &config);
        assert_eq!(
            normalized
                .tags
                .iter()
                .filter(|tag| tag.as_str() == "shared")
                .count(),
            1
        );
    }
}
