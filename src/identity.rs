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
        assert_eq!(source_id, format!("ns|urn:sha1:{}", sha1_hex("content")));
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
        let expected = format!(
            "urn:sha1:{}",
            sha1_hex("title:Title|published:10|updated:0|author:A")
        );
        assert_eq!(fallback, expected);
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
        let expected = format!(
            "urn:sha1:{}",
            sha1_hex("title:|published:0|updated:0|author:")
        );
        assert_eq!(fallback, expected);
    }
}
