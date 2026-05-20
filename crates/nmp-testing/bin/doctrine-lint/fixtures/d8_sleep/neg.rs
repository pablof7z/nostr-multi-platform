//! Negative D8 (no-polling) fixture — must report ZERO findings.
//!
//! Demonstrates every legitimate exemption:
//!   1. production code that blocks on `recv` instead of sleeping;
//!   2. a `thread::sleep` / `tokio::time::sleep` inside a `#[cfg(test)] mod
//!      tests` block (test timing helpers may sleep — `in_test_cfg`
//!      exempts them);
//!   3. a `thread::sleep` / `tokio::time::sleep_until` with an explicit
//!      `// doctrine-allow: D8` override.

use std::sync::mpsc::Receiver;

/// Correct pattern: block on the channel, no polling.
pub fn wait_for_signal(rx: &Receiver<()>) {
    // Blocking `recv` — the D8-compliant way to wait. No `thread::sleep`.
    let _ = rx.recv();
}

/// Genuine one-off need, justified inline. The override suppresses D8.
pub fn deliberate_pause() {
    std::thread::sleep(std::time::Duration::from_millis(1)); // doctrine-allow: D8 — hardware settle delay, not a poll loop
}

/// Async one-off need, justified inline. The override suppresses D8.
pub async fn deliberate_async_pause(deadline: tokio::time::Instant) {
    tokio::time::sleep_until(deadline).await; // doctrine-allow: D8 — hardware settle delay, not a poll loop
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_timing_helper_may_sleep() {
        // Inside `#[cfg(test)]` — test code legitimately sleeps to let a
        // spawned thread make progress. `in_test_cfg` exempts this.
        thread::sleep(Duration::from_millis(10));
    }

    #[tokio::test]
    async fn async_test_timing_helper_may_sleep() {
        // Inside `#[cfg(test)]` — test code legitimately awaits a sleep.
        // `in_test_cfg` exempts the async form too.
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
