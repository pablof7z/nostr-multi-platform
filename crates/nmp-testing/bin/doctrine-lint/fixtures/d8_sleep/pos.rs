//! Positive D8 (no-polling) fixture — production code that busy-waits with
//! `thread::sleep` or its async `tokio::time::sleep` equivalents. Each call
//! below MUST be flagged as a D8 violation.

use std::thread;
use std::time::Duration;

/// A polling loop: sleep, check, repeat. The canonical D8 violation.
pub fn wait_for_ready(is_ready: impl Fn() -> bool) {
    while !is_ready() {
        // Bare `thread::sleep` after `use std::thread;` — must be flagged.
        thread::sleep(Duration::from_millis(50));
    }
}

/// Fully-qualified form — must also be flagged.
pub fn backoff_then_retry() {
    std::thread::sleep(Duration::from_millis(200));
}

/// Async poll: `tokio::time::sleep` is the await-based equivalent of a
/// busy-wait — equally banned under D8.
pub async fn async_wait_for_ready(is_ready: impl Fn() -> bool) {
    while !is_ready() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Deadline-based async sleep — `tokio::time::sleep_until` is also a poll.
pub async fn async_backoff_until(deadline: tokio::time::Instant) {
    tokio::time::sleep_until(deadline).await;
}
