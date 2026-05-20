//! Positive D8 (no-polling) fixture — production code that busy-waits with
//! `thread::sleep`. Each call below MUST be flagged as a D8 violation.

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
