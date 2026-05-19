//! Live firehose-bench scenarios — real WebSocket I/O against production relays.
//!
//! Per-scenario files:
//! - `cold_start.rs`        — time-to-first-item + filled-timeline gates
//! - `profile_thrashing.rs` — claim/release dedup ratio + leak gate

mod cold_start;
mod profile_thrashing;

pub(crate) use cold_start::cold_start;
pub(crate) use profile_thrashing::profile_thrashing;

use crate::report::ScenarioResult;
use serde_json::Value;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

// ── gate and timing constants (shared between scenarios) ──────────────────────

/// Maximum time to wait for the relay to connect before giving up.
pub(super) const WARMUP_TIMEOUT: Duration = Duration::from_secs(30);

// ── helper functions ──────────────────────────────────────────────────────────

/// Drain the update channel and return the newest JSON string received.
pub(super) fn drain(rx: &Receiver<String>) -> Option<String> {
    let mut latest = None;
    while let Ok(update) = rx.try_recv() {
        latest = Some(update);
    }
    latest
}

/// Wait up to `ceiling` for the first update to arrive, then drain any
/// additional queued updates, returning the newest.
///
/// The burst-then-quiesce pattern of `profile_thrashing` means the actor may
/// not have pushed a snapshot yet when `drain()` is called bare; a short
/// `sleep` before `drain()` races the actor.  This function removes the race
/// by blocking until at least one update arrives.
///
/// Prints a warning to stderr and returns `None` when the ceiling elapses
/// with no update — callers must treat `None` as `snapshot_valid = false`.
pub(super) fn drain_until(rx: &Receiver<String>, ceiling: Duration) -> Option<String> {
    let deadline = Instant::now() + ceiling;
    let first = wait_update(rx, deadline);
    if first.is_none() {
        eprintln!("drain timeout — snapshot may be stale; gate will fail closed");
        return None;
    }
    // Drain any additional updates that arrived while we waited.
    Some(drain(rx).unwrap_or_else(|| first.unwrap()))
}

/// Block until a new update arrives or the deadline passes.
/// Returns `None` only when the deadline has been reached.
pub(super) fn wait_update(rx: &Receiver<String>, deadline: Instant) -> Option<String> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            return drain(rx);
        }
        match rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(update) => return Some(update),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(latest) = drain(rx) {
                    return Some(latest);
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// Extract a `f64` from `update["metrics"][field]`.
pub(super) fn metric(update: &str, field: &str) -> Option<f64> {
    let value: Value = serde_json::from_str(update).ok()?;
    value.get("metrics")?.get(field)?.as_f64()
}

/// Count open (non-closed) wire subscriptions in the update JSON.
pub(super) fn open_sub_count(update: &str) -> usize {
    let Ok(value) = serde_json::from_str::<Value>(update) else {
        return 0;
    };
    value
        .get("wire_subscriptions")
        .and_then(Value::as_array)
        .map(|subs| {
            subs.iter()
                .filter(|sub| {
                    !matches!(
                        sub.get("state").and_then(Value::as_str),
                        Some("closed") | Some("closed_by_relay")
                    )
                })
                .count()
        })
        .unwrap_or(0)
}

/// Wait for the relay connection field to read "connected".
pub(super) fn wait_connected(rx: &Receiver<String>) -> bool {
    let deadline = Instant::now() + WARMUP_TIMEOUT;
    loop {
        let Some(update) = wait_update(rx, deadline) else {
            return false;
        };
        if update.contains("\"connection\":\"connected\"") {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
    }
}

pub(super) fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// Type alias for sub-module use.
pub(super) type Scenario = ScenarioResult;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;

    /// Regression: sender arrives after 500 ms (past the old 300 ms sleep window).
    /// `drain_until` with a 2 s ceiling must still return `Some`.
    #[test]
    fn drain_until_waits_for_delayed_sender() {
        let (tx, rx) = mpsc::channel::<String>();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            tx.send(r#"{"wire_subscriptions":[]}"#.to_string()).unwrap();
        });
        let result = drain_until(&rx, Duration::from_secs(2));
        assert!(
            result.is_some(),
            "drain_until must return Some when update arrives within ceiling"
        );
    }

    /// No sender: `drain_until` must return `None` after the ceiling elapses.
    #[test]
    fn drain_until_returns_none_on_timeout() {
        let (_tx, rx) = mpsc::channel::<String>();
        // Short ceiling so the test completes quickly.
        let result = drain_until(&rx, Duration::from_millis(100));
        assert!(
            result.is_none(),
            "drain_until must return None when no update arrives before ceiling"
        );
    }

    /// Multiple rapid updates: `drain_until` must return the latest, not the first.
    #[test]
    fn drain_until_returns_latest_when_multiple_updates_queued() {
        let (tx, rx) = mpsc::channel::<String>();
        // Pre-fill the channel before calling drain_until.
        tx.send("first".to_string()).unwrap();
        tx.send("second".to_string()).unwrap();
        tx.send("third".to_string()).unwrap();
        let result = drain_until(&rx, Duration::from_secs(1));
        assert_eq!(
            result.as_deref(),
            Some("third"),
            "drain_until must return the newest update"
        );
    }
}
