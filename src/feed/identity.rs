//! Feed identity helpers.

use hex::ToHex;
use sha2::{Digest, Sha256};

/// Generates a stable feed key from the feed URL.
pub fn feed_key_from_url(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    digest.encode_hex::<String>()
}
