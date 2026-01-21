//! Time utilities.

/// Returns current epoch seconds.
pub fn current_epoch() -> i64 {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
    duration.as_secs() as i64
}
