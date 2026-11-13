//! Identity helpers for feeds and entries.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// Returns a URL-safe base64-encoded SHA-256 digest without padding.
pub fn sha256_base64url_nopad(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("k_{}", URL_SAFE_NO_PAD.encode(hasher.finalize()))
}

/// Input data used to derive a stable entry identity.
struct EntryIdentityInput<'a> {
    /// Namespace used to disambiguate identifiers across feeds.
    pub namespace: &'a str,
    /// Feed-provided identifier (Atom id or RSS guid).
    pub raw_id: Option<&'a str>,
    /// Entry link when no raw id is provided.
    pub link: Option<&'a str>,
    /// Best available content payload for deterministic fallback hashing.
    pub content: Option<&'a str>,
    /// Entry title for fallback hashing.
    pub title: Option<&'a str>,
    /// Published timestamp for fallback hashing.
    pub published_at: Option<i64>,
    /// Updated timestamp for fallback hashing.
    pub updated_at: Option<i64>,
    /// Author name for fallback hashing.
    pub author: Option<&'a str>,
}

/// Builds an entry id from a feed entry and optional content payload.
pub(crate) fn entry_id_from_entry(
    feed_id: &str,
    entry: &feed_rs::model::Entry,
    content: Option<&str>,
) -> String {
    let raw_id = if entry.id.is_empty() {
        None
    } else {
        Some(entry.id.as_str())
    };
    let link = entry
        .links
        .iter()
        .map(|link| link.href.as_str())
        .find(|href| !href.trim().is_empty());
    let title = entry.title.as_ref().map(|title| title.content.as_str());
    let author = entry.authors.first().map(|author| author.name.as_str());
    let published_at = entry.published.map(|value| value.timestamp());
    let updated_at = entry.updated.map(|value| value.timestamp());
    let identity = EntryIdentityInput {
        namespace: feed_id,
        raw_id,
        link,
        content,
        title,
        published_at,
        updated_at,
        author,
    };
    let source_id = build_entry_source_id(&identity);
    build_entry_id(identity.namespace, &source_id)
}

/// Normalizes identifiers by trimming and collapsing whitespace.
fn cleanup_identifier(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Returns a normalized identifier when present.
fn normalized_identifier(value: Option<&str>) -> Option<String> {
    value
        .map(cleanup_identifier)
        .filter(|value| !value.is_empty())
}

/// Builds a canonical source_id using namespace and best available identifier.
fn build_entry_source_id(identity: &EntryIdentityInput<'_>) -> String {
    if let Some(id) = normalized_identifier(identity.raw_id) {
        return format!("{}|{}", identity.namespace, id);
    }
    if let Some(link) = normalized_identifier(identity.link) {
        return format!("{}|{}", identity.namespace, link);
    }
    let fallback = build_fallback_id(identity);
    format!("{}|{}", identity.namespace, fallback)
}

/// Builds a stable entry id from feed id and source_id.
fn build_entry_id(feed_id: &str, source_id: &str) -> String {
    sha256_base64url_nopad(&format!("{feed_id}:{source_id}"))
}

/// Builds a deterministic fallback ID when feed identifiers are missing.
fn build_fallback_id(identity: &EntryIdentityInput<'_>) -> String {
    if let Some(content) = identity.content.filter(|value| !value.is_empty()) {
        return format!("urn:sha1:{}", sha1_hex(content));
    }
    let title = identity.title.unwrap_or("");
    let author = identity.author.unwrap_or("");
    let published = identity.published_at.unwrap_or(0);
    let updated = identity.updated_at.unwrap_or(0);
    let seed = format!("title:{title}|published:{published}|updated:{updated}|author:{author}");
    format!("urn:sha1:{}", sha1_hex(&seed))
}

/// Computes SHA1 hex digest for a string.
fn sha1_hex(value: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_atom_entry(fields: &str) -> feed_rs::model::Entry {
        let xml = format!(
            r#"<feed xmlns="http://www.w3.org/2005/Atom">
                <id>urn:feed:test</id><title>Feed</title>
                <updated>2020-01-02T03:04:05Z</updated>
                <entry>{fields}</entry>
            </feed>"#
        );
        feed_rs::parser::parse(xml.as_bytes())
            .expect("parse Atom")
            .entries
            .remove(0)
    }

    #[test]
    fn parsed_guid_is_scoped_to_feed_and_ignores_other_entry_fields() {
        let xml = r#"<rss version="2.0"><channel>
            <title>Feed</title><link>https://example.com</link><description>Feed</description>
            <item><guid isPermaLink="false">stable-guid</guid><title>Entry</title>
                <link>https://example.com/entry</link>
                <pubDate>Thu, 02 Jan 2020 03:04:05 GMT</pubDate>
                <description>content</description>
            </item>
        </channel></rss>"#;
        let mut entry = feed_rs::parser::parse(xml.as_bytes())
            .expect("parse RSS")
            .entries
            .remove(0);
        assert_eq!(entry.id, "stable-guid");
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, Some("content")),
            "k_amcDNa68q9wb-Qyz1DP4CBhrmq74FdrR12iI02k1C64"
        );

        entry.title.as_mut().expect("title").content = "Changed".to_string();
        entry.links.clear();
        entry.published = None;
        entry.updated = None;
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, Some("changed content")),
            "k_amcDNa68q9wb-Qyz1DP4CBhrmq74FdrR12iI02k1C64"
        );
        assert_eq!(
            entry_id_from_entry("feed-b", &entry, Some("changed content")),
            "k_TBQM1Ht-cRuhNix_DDIB2VLdN9_R1Byw7KOyzSN8pFE"
        );
    }

    #[test]
    fn parsed_rss_without_guid_uses_link_and_title() {
        let id_for = |title: &str, link: &str| {
            let xml = format!(
                r#"<rss version="2.0"><channel>
                    <title>Feed</title><link>https://example.com</link><description>Feed</description>
                    <item><title>{title}</title><link>{link}</link><description>content</description></item>
                </channel></rss>"#
            );
            let entry = feed_rs::parser::parse(xml.as_bytes())
                .expect("parse RSS")
                .entries
                .remove(0);
            entry_id_from_entry("feed-a", &entry, Some("content"))
        };
        let original = id_for("Entry", "https://example.com/entry");
        assert_eq!(original, "k_DoV2SQGtIbQt_7uR5TV7DGKnIM2mCLtqlBZCn8MFumA");
        assert_eq!(original, id_for("Entry", "https://example.com/entry"));
        assert_ne!(original, id_for("Changed", "https://example.com/entry"));
        assert_ne!(original, id_for("Entry", "https://example.com/other"));
    }

    #[test]
    fn parser_supplied_id_for_empty_atom_id_precedes_application_fallback() {
        let mut entry = parse_atom_entry(
            r#"<id></id><title>Entry</title><published>2020-01-01T01:02:03Z</published>
                <updated>2020-01-02T03:04:05Z</updated><content type="text">content</content>"#,
        );
        assert!(!entry.id.trim().is_empty(), "parser must supply an ID");
        let original = entry_id_from_entry("feed-a", &entry, Some("content"));
        entry.title.as_mut().expect("title").content = "Changed".to_string();
        entry.content.as_mut().expect("content").body = Some("changed content".to_string());
        entry.published = None;
        entry.updated = None;
        assert_eq!(
            original,
            entry_id_from_entry("feed-a", &entry, Some("changed content"))
        );
    }

    #[test]
    fn whitespace_atom_id_uses_link_before_content_fallback() {
        let mut entry = parse_atom_entry(
            r#"<id>   </id><title>Entry</title><updated>2020-01-02T03:04:05Z</updated>
                <link href="https://example.com/entry"/><content type="text">content</content>"#,
        );
        assert!(!entry.id.is_empty());
        assert!(entry.id.trim().is_empty());
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, Some("content")),
            "k_o-yRTzrO9X74bqiA8UJ79stwAiTZcW1bkl3L2ZqeBi0"
        );
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, Some("changed content")),
            "k_o-yRTzrO9X74bqiA8UJ79stwAiTZcW1bkl3L2ZqeBi0"
        );
        entry.links[0].href = "https://example.com/other".to_string();
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, Some("content")),
            "k_JPFxD7PMCNU0lZLceVjJVCyCNj0m-TiLeSAFn6oAkBw"
        );
    }

    #[test]
    fn whitespace_atom_id_without_link_uses_content_before_metadata() {
        let mut entry = parse_atom_entry(
            r#"<id>   </id><title>Entry</title><updated>2020-01-02T03:04:05Z</updated>
                <content type="text">content</content>"#,
        );
        assert!(!entry.id.is_empty());
        assert!(entry.id.trim().is_empty());
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, Some("content")),
            "k_dcr8rO1ZwcRow3hq8jPYjmKwTKGaB-uPCSEK7X8Jw4Q"
        );
        entry.title.as_mut().expect("title").content = "Changed".to_string();
        entry.updated = None;
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, Some("content")),
            "k_dcr8rO1ZwcRow3hq8jPYjmKwTKGaB-uPCSEK7X8Jw4Q"
        );
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, Some("changed content")),
            "k_zIKW2PCUY79QDMxKDrMKK9U8EYTTbIitvsz452vmMQo"
        );
    }

    #[test]
    fn whitespace_atom_id_without_link_or_content_uses_metadata() {
        let mut entry = parse_atom_entry(
            r#"<id>   </id><title>Entry</title><published>2020-01-01T01:02:03Z</published>
                <updated>2020-01-02T03:04:05Z</updated><author><name>Alice</name></author>"#,
        );
        assert!(!entry.id.is_empty());
        assert!(entry.id.trim().is_empty());
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, None),
            "k_kzrYvAWieACobaMR9_iG-okVap63b6qvXw1ttrCgGlo"
        );
        entry.title.as_mut().expect("title").content = "Changed".to_string();
        assert_eq!(
            entry_id_from_entry("feed-a", &entry, None),
            "k_Ga20Qk7xdeFbPEpCBS2aIOiwXJgV04aM0jvYMuLFUdg"
        );
    }

    #[test]
    fn build_entry_source_id_uses_namespace_and_cleaned_id() {
        let identity = EntryIdentityInput {
            namespace: "ns",
            raw_id: Some("  hello \n world "),
            link: None,
            content: None,
            title: None,
            published_at: None,
            updated_at: None,
            author: None,
        };
        let source_id = build_entry_source_id(&identity);
        assert_eq!(source_id, "ns|hello world");
    }

    #[test]
    fn build_entry_source_id_uses_content_hash_when_identifiers_missing() {
        let identity = EntryIdentityInput {
            namespace: "ns",
            raw_id: None,
            link: None,
            content: Some("content"),
            title: None,
            published_at: None,
            updated_at: None,
            author: None,
        };
        let source_id = build_entry_source_id(&identity);
        assert_eq!(
            source_id,
            "ns|urn:sha1:040f06fd774092478d450774f5ba30c5da78acc8"
        );
    }

    #[test]
    fn build_fallback_id_is_deterministic_without_content() {
        let identity = EntryIdentityInput {
            namespace: "ns",
            raw_id: None,
            link: None,
            content: Some(""),
            title: Some("Title"),
            published_at: Some(10),
            updated_at: None,
            author: Some("A"),
        };
        let fallback = build_fallback_id(&identity);
        assert_eq!(
            fallback,
            "urn:sha1:1b2151838a0a4e6e052366d7c3d816fe9d272a8c"
        );
    }

    #[test]
    fn build_fallback_id_is_deterministic_with_empty_seed() {
        let identity = EntryIdentityInput {
            namespace: "ns",
            raw_id: None,
            link: None,
            content: None,
            title: None,
            published_at: None,
            updated_at: None,
            author: None,
        };
        let fallback = build_fallback_id(&identity);
        assert_eq!(
            fallback,
            "urn:sha1:70563464e467556b94689c3f2a7ca8d8bff34caa"
        );
    }
}
