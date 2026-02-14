//! Feed identity helpers.

/// Generates a stable public feed id from the feed URL.
pub fn feed_id_from_url(url: &str) -> String {
    crate::identity::sha256_base64url_nopad(url)
}
