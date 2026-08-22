//! Entry normalization pipeline.

use crate::config::AppConfig;
use crate::db::EntryEnclosureInput;
use crate::identity::entry_id_from_entry;
use crate::string_set::dedupe_strings_preserve_order;
use crate::time::current_epoch;
use std::collections::HashSet;

use super::autotag::match_auto_tags;
use super::content::{build_entry_content, select_content};
use super::model::{PendingEntry, SyncEntry, SyncTarget};

fn entry_meta_json(entry: &feed_rs::model::Entry) -> Option<String> {
    let mut metadata = serde_json::Map::new();
    if !entry.authors.is_empty() {
        let authors = entry
            .authors
            .iter()
            .map(|author| {
                let mut fields = serde_json::Map::new();
                fields.insert(
                    "name".to_string(),
                    serde_json::Value::String(author.name.clone()),
                );
                if let Some(uri) = &author.uri {
                    fields.insert("uri".to_string(), serde_json::Value::String(uri.clone()));
                }
                if let Some(email) = &author.email {
                    fields.insert(
                        "email".to_string(),
                        serde_json::Value::String(email.clone()),
                    );
                }
                serde_json::Value::Object(fields)
            })
            .collect();
        metadata.insert("authors".to_string(), serde_json::Value::Array(authors));
    }
    if !entry.categories.is_empty() {
        metadata.insert(
            "categories".to_string(),
            serde_json::Value::Array(
                entry
                    .categories
                    .iter()
                    .map(|category| serde_json::Value::String(category.term.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(base) = &entry.base {
        metadata.insert(
            "base-url".to_string(),
            serde_json::Value::String(base.clone()),
        );
    }
    (!metadata.is_empty()).then(|| serde_json::Value::Object(metadata).to_string())
}

/// Normalizes feed enclosure representations into the persisted fields.
pub(crate) fn normalize_enclosures(
    entry: &feed_rs::model::Entry,
    feed_type: &feed_rs::model::FeedType,
) -> Vec<EntryEnclosureInput> {
    let mut seen = HashSet::new();
    let mut enclosures = Vec::new();
    let mut push = |url: String, mime_type: Option<String>, length: Option<u64>| {
        if seen.insert(url.clone()) {
            enclosures.push(EntryEnclosureInput {
                url,
                mime_type,
                length: length.and_then(|value| i64::try_from(value).ok()),
            });
        }
    };
    for link in &entry.links {
        // feed-rs 1.5.3 maps JSON Feed attachments to links with no rel and a media type;
        // RSS enclosures use media, while Atom links without rel default to alternate.
        if link.rel.as_deref() == Some("enclosure")
            || (feed_type == &feed_rs::model::FeedType::JSON
                && link.rel.is_none()
                && link.media_type.is_some())
        {
            push(link.href.clone(), link.media_type.clone(), link.length);
        }
    }
    for media in &entry.media {
        for content in &media.content {
            if let Some(url) = &content.url {
                push(
                    url.to_string(),
                    content.content_type.as_ref().map(ToString::to_string),
                    content.size,
                );
            }
        }
    }
    enclosures
}

/// Normalizes a feed entry into database payloads.
pub(crate) fn normalize_entry(
    entry: &feed_rs::model::Entry,
    target: &SyncTarget,
    config: &AppConfig,
    feed_type: &feed_rs::model::FeedType,
) -> SyncEntry {
    let link = entry.links.first().map(|link| link.href.clone());
    let title = entry.title.as_ref().map(|title| title.content.clone());
    let author = entry.authors.first().map(|author| author.name.clone());
    let published_at = entry.published.map(|value| value.timestamp());
    let updated_at = entry.updated.map(|value| value.timestamp());
    let first_seen_at = current_epoch();

    let (content, content_type) = select_content(entry);
    let entry_id = entry_id_from_entry(&target.ctx.feed_id, entry, content.as_deref());
    let content_plan = build_entry_content(config.storage.content_store, content, content_type);

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
    let meta_json = entry_meta_json(entry);

    SyncEntry {
        entry: PendingEntry {
            entry_id,
            link,
            title,
            author,
            published_at,
            updated_at,
            first_seen_at,
            meta_json,
        },
        content: content_plan.content,
        content_payload: content_plan.payload,
        enclosures: normalize_enclosures(entry, feed_type),
        tags,
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_enclosures, normalize_entry};
    use crate::config::feeds::AutoTagRule;
    use crate::config::{
        AppConfig, CliConfig, ContentStore, DatabaseConfig, FeedsSourceConfig, QueryConfig,
        StorageConfig, SyncConfig,
    };
    use crate::sync::autotag::compile_auto_tags;
    use crate::sync::model::{FeedContext, SyncTarget};
    use feed_rs::model::FeedType;
    use std::io::Cursor;
    use std::path::PathBuf;

    #[test]
    fn normalize_enclosures_maps_atom_enclosure_links() {
        let feed = feed_rs::parser::parse(Cursor::new(
            r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <id>https://example.com/feed</id>
  <title>Test feed</title>
  <entry>
    <id>entry-1</id>
    <title>Entry</title>
    <link rel="enclosure" href="https://example.com/audio.mp3" type="audio/mpeg" length="42" />
  </entry>
</feed>"#,
        ))
        .expect("parse feed");

        let enclosures = normalize_enclosures(&feed.entries[0], &feed.feed_type);

        assert_eq!(enclosures.len(), 1);
        assert_eq!(enclosures[0].url, "https://example.com/audio.mp3");
        assert_eq!(enclosures[0].mime_type.as_deref(), Some("audio/mpeg"));
        assert_eq!(enclosures[0].length, Some(42));
    }

    #[test]
    fn normalize_enclosures_maps_rss_and_media_content() {
        let feed = feed_rs::parser::parse(Cursor::new(
            r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
  <channel><title>Test feed</title><link>https://example.com</link>
    <item>
      <guid>entry-1</guid><title>Entry</title><link>https://example.com/entry</link>
      <enclosure url="https://example.com/audio.mp3" type="audio/mpeg" length="42" />
      <media:content url="https://example.com/video.mp4" type="video/mp4" fileSize="99" />
    </item>
  </channel>
</rss>"#,
        ))
        .expect("parse feed");

        let enclosures = normalize_enclosures(&feed.entries[0], &feed.feed_type);

        assert_eq!(enclosures.len(), 2);
        assert_eq!(enclosures[0].url, "https://example.com/audio.mp3");
        assert_eq!(enclosures[0].mime_type.as_deref(), Some("audio/mpeg"));
        assert_eq!(enclosures[0].length, Some(42));
        assert_eq!(enclosures[1].url, "https://example.com/video.mp4");
        assert_eq!(enclosures[1].mime_type.as_deref(), Some("video/mp4"));
        assert_eq!(enclosures[1].length, Some(99));
    }

    #[test]
    fn normalize_enclosures_maps_json_feed_attachments_and_deduplicates_urls() {
        let feed = feed_rs::parser::parse(Cursor::new(
            r#"{
  "version": "https://jsonfeed.org/version/1.1",
  "title": "Test feed",
  "items": [{
    "id": "entry-1",
    "content_text": "Entry",
    "attachments": [
      {"url": "https://example.com/audio.mp3", "mime_type": "audio/mpeg", "size_in_bytes": 42},
      {"url": "https://example.com/audio.mp3", "mime_type": "audio/other", "size_in_bytes": 43}
    ]
  }]
}"#,
        ))
        .expect("parse feed");

        let enclosures = normalize_enclosures(&feed.entries[0], &feed.feed_type);

        assert_eq!(enclosures.len(), 1);
        assert_eq!(enclosures[0].url, "https://example.com/audio.mp3");
        assert_eq!(enclosures[0].mime_type.as_deref(), Some("audio/mpeg"));
        assert_eq!(enclosures[0].length, Some(42));
    }

    #[test]
    fn normalize_enclosures_does_not_infer_media_links_outside_json_feed() {
        let entry = feed_rs::model::Entry {
            links: vec![feed_rs::model::Link {
                href: "https://example.com/audio.mp3".to_string(),
                rel: None,
                media_type: Some("audio/mpeg".to_string()),
                href_lang: None,
                title: None,
                length: Some(42),
            }],
            ..feed_rs::model::Entry::default()
        };

        assert!(normalize_enclosures(&entry, &FeedType::Atom).is_empty());
    }

    #[test]
    fn normalize_enclosures_omits_lengths_that_do_not_fit_sqlite_integer() {
        let mut entry = feed_rs::model::Entry::default();
        entry.links.push(feed_rs::model::Link {
            href: "https://example.com/large.bin".to_string(),
            rel: Some("enclosure".to_string()),
            media_type: Some("application/octet-stream".to_string()),
            href_lang: None,
            title: None,
            length: Some(u64::MAX),
        });

        let enclosures = normalize_enclosures(&entry, &FeedType::RSS2);

        assert_eq!(enclosures[0].length, None);
    }

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
            normalize_entry(
                &feed.entries[0],
                &target_only,
                &unread_disabled,
                &feed.feed_type
            )
            .tags
            .iter()
            .any(|tag| tag.as_str() == "target")
        );

        let mut auto_only = target.clone();
        auto_only.tags.clear();
        assert!(
            normalize_entry(
                &feed.entries[0],
                &auto_only,
                &unread_disabled,
                &feed.feed_type
            )
            .tags
            .iter()
            .any(|tag| tag.as_str() == "shared")
        );

        let mut unread_only = target.clone();
        unread_only.tags.clear();
        unread_only.auto_tag_rules.clear();
        assert!(
            normalize_entry(&feed.entries[0], &unread_only, &config, &feed.feed_type)
                .tags
                .iter()
                .any(|tag| tag.as_str() == "shared")
        );

        let normalized = normalize_entry(&feed.entries[0], &target, &config, &feed.feed_type);
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
