//! Time utilities.

/// Returns current epoch seconds.
///
/// # Panics
///
/// Panics when the system time is before the Unix epoch.
pub fn current_epoch() -> i64 {
    current_epoch_at(std::time::SystemTime::now())
}

fn current_epoch_at(now: std::time::SystemTime) -> i64 {
    now.duration_since(std::time::UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::current_epoch_at;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    #[should_panic(expected = "system time before unix epoch")]
    fn current_epoch_rejects_time_before_unix_epoch() {
        current_epoch_at(UNIX_EPOCH - Duration::from_secs(1));
    }
}
