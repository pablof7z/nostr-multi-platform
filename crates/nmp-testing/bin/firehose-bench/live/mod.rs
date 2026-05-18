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
