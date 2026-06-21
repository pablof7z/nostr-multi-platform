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
use nmp_core::decode_snapshot_envelope;
use nmp_testing::harness_probe::{drain_latest, recv_latest_until};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

// ── gate and timing constants (shared between scenarios) ──────────────────────

/// Maximum time to wait for the relay to connect before giving up.
pub(super) const WARMUP_TIMEOUT: Duration = Duration::from_secs(30);

// ── helper functions ──────────────────────────────────────────────────────────

/// Drain the update channel and return the newest FlatBuffers frame received.
pub(super) fn drain(rx: &Receiver<Vec<u8>>) -> Option<Vec<u8>> {
    drain_latest(rx)
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
pub(super) fn drain_until(rx: &Receiver<Vec<u8>>, ceiling: Duration) -> Option<Vec<u8>> {
    let deadline = Instant::now() + ceiling;
    match wait_update(rx, deadline) {
        Some(latest) => Some(latest),
        None => {
            eprintln!("drain timeout — snapshot may be stale; gate will fail closed");
            None
        }
    }
}

/// Block until a new update arrives or the deadline passes, returning the newest
/// queued frame. Returns `None` only when the deadline is reached with no frame
/// or the actor disconnected before one arrived.
///
/// Backed by `recv_latest_until`, which blocks on the actual deadline rather
/// than waking on a fixed interval; after the first frame it collapses any
/// immediately-queued updates into the newest (older snapshots are superseded).
pub(super) fn wait_update(rx: &Receiver<Vec<u8>>, deadline: Instant) -> Option<Vec<u8>> {
    recv_latest_until(rx, deadline)
}

/// Extract the typed `visible_items` metric off the Tier-3 envelope
/// (PR-B: the generic JSON payload no longer exists on the wire).
pub(super) fn visible_items(update: &[u8]) -> Option<u64> {
    Some(decode_snapshot_envelope(update).ok()?.visible_items)
}

/// Count open (non-closed) wire subscriptions in the typed Tier-3 envelope.
pub(super) fn open_sub_count(update: &[u8]) -> usize {
    let Ok(envelope) = decode_snapshot_envelope(update) else {
        return 0;
    };
    envelope
        .wire_subscriptions
        .iter()
        .filter(|sub| !matches!(sub.state.as_str(), "closed" | "closed_by_relay"))
        .count()
}

/// Wait for the typed `relay_status` aggregate to read "connected".
pub(super) fn wait_connected(rx: &Receiver<Vec<u8>>) -> bool {
    let deadline = Instant::now() + WARMUP_TIMEOUT;
    loop {
        let Some(update) = wait_update(rx, deadline) else {
            return false;
        };
        if decode_snapshot_envelope(&update)
            .ok()
            .and_then(|envelope| envelope.relay_status)
            .map(|aggregate| aggregate.connection == "connected")
            .unwrap_or(false)
        {
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
    use nmp_core::{encode_snapshot_frame, SnapshotEnvelope};
    use std::sync::mpsc;
    use std::thread;

    /// Encode a minimal typed frame whose `rev` marks the fixture's identity
    /// (PR-B: arbitrary JSON trees no longer exist on the wire).
    fn update_fixture(rev: u64) -> Vec<u8> {
        encode_snapshot_frame(
            &SnapshotEnvelope {
                rev,
                ..Default::default()
            },
            &[],
        )
    }

    /// Regression: sender arrives after 500 ms (past the old 300 ms sleep window).
    /// `drain_until` with a 2 s ceiling must still return `Some`.
    #[test]
    fn drain_until_waits_for_delayed_sender() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            tx.send(update_fixture(1)).unwrap();
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
        let (_tx, rx) = mpsc::channel::<Vec<u8>>();
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
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        // Pre-fill the channel before calling drain_until.
        tx.send(update_fixture(1)).unwrap();
        tx.send(update_fixture(2)).unwrap();
        tx.send(update_fixture(3)).unwrap();
        let result = drain_until(&rx, Duration::from_secs(1));
        assert_eq!(
            result
                .as_deref()
                .and_then(|bytes| decode_snapshot_envelope(bytes).ok())
                .map(|envelope| envelope.rev),
            Some(3),
            "drain_until must return the newest update"
        );
    }
}
