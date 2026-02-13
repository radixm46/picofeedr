//! Feed identity helpers.

/// Generates a stable feed key from the feed URL.
pub fn feed_key_from_url(url: &str) -> String {
    crate::identity::sha256_base64url_nopad(url)
}
