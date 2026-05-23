//! Relay keepalive FSM (T120b / G4).
//!
//! Per-socket idle-detector that drives WebSocket Ping/Pong without depending
//! on wall-clock time. The worker calls `on_inbound` whenever it reads from the
//! socket (any frame counts — Pong included), then asks `step(now)` each tick
//! whether to emit a Ping, give up, or do nothing.
//!
//! ## State machine
//!
//! - **Quiet** — `last_inbound_at` aged < `idle_threshold`. Nothing to do.
//! - **`PingPending`** — emitted a Ping at `ping_sent_at`. Waiting for a Pong (or
//!   any inbound frame; a relay that's alive must respond to *something*).
//! - **Dead** — `ping_sent_at` aged past `pong_timeout`. Caller marks the
//!   connection unhealthy and reconnects.
//!
//! ## Why this is a pure FSM
//!
//! Time enters through caller-supplied `Instant` only — no `Instant::now()`
//! inside. Pong arrival is signalled by `on_inbound` (since tungstenite delivers
//! Pongs as regular reads). This keeps tests deterministic and the worker
//! integration trivial.
//!
//! ## Defaults
//!
//! Per the T120b spec: 30s idle → emit Ping, 30s no response → unhealthy.
//! These are knobs on `KeepaliveState::new`; production wires the constants
//! at the `relay_worker` layer.
//!
//! ## D7 — actor decides
//!
//! This module does NOT close the socket. `step` returns a verdict; the worker
//! decides what to do with it. `Dead` triggers a `RelayEvent::Failed` plus
//! reconnect via the existing T116 path.
//!
//! ## D6 — internal types only
//!
//! Every type here is `pub(crate)` or below; nothing leaks across the FFI
//! boundary.

use std::time::{Duration, Instant};

/// Verdict returned by `KeepaliveState::step`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeepaliveAction {
    /// Nothing to do — socket is healthy and not idle past the threshold.
    Idle,
    /// Emit a `Message::Ping(vec![])` on the wire and stamp `ping_sent_at`.
    EmitPing,
    /// Pong window elapsed with no inbound traffic. Caller marks the socket
    /// failed and reconnects.
    Dead,
}

/// Pure per-socket keepalive driver. Owned by `run_connected_relay`; reset on
/// every reconnect (a fresh socket starts in `Quiet` with `last_inbound_at`
/// pinned to the connect moment).
pub(crate) struct KeepaliveState {
    idle_threshold: Duration,
    pong_timeout: Duration,
    last_inbound_at: Instant,
    ping_sent_at: Option<Instant>,
}

impl KeepaliveState {
    /// Build a fresh driver. `now` is the socket-open moment so the first
    /// `idle_threshold` worth of silence post-connect is tolerated without a
    /// premature ping. Production passes the same `Instant` it just read for
    /// the `Connected` event.
    #[must_use]
    pub(crate) fn new(now: Instant, idle_threshold: Duration, pong_timeout: Duration) -> Self {
        Self {
            idle_threshold,
            pong_timeout,
            last_inbound_at: now,
            ping_sent_at: None,
        }
    }

    /// Any inbound frame — including a Pong reply to our Ping. Resets both
    /// the idle clock and any outstanding pong wait.
    pub(crate) fn on_inbound(&mut self, now: Instant) {
        self.last_inbound_at = now;
        self.ping_sent_at = None;
    }

    /// Step the FSM. Caller supplies `now`; we never read the wall clock.
    pub(crate) fn step(&mut self, now: Instant) -> KeepaliveAction {
        // Pong window first — if a ping is outstanding and the window has
        // elapsed without an inbound frame, the socket is dead.
        if let Some(sent_at) = self.ping_sent_at {
            if now.saturating_duration_since(sent_at) >= self.pong_timeout {
                return KeepaliveAction::Dead;
            }
            // Ping in flight, window not yet elapsed — wait.
            return KeepaliveAction::Idle;
        }

        // No ping outstanding. Have we been silent long enough to emit one?
        if now.saturating_duration_since(self.last_inbound_at) >= self.idle_threshold {
            self.ping_sent_at = Some(now);
            return KeepaliveAction::EmitPing;
        }

        KeepaliveAction::Idle
    }

    /// Next wall-clock instant at which [`step`] can change state without an
    /// inbound frame. Callers use this as a blocking deadline, not a poll rate.
    pub(crate) fn next_deadline(&self) -> Instant {
        if let Some(sent_at) = self.ping_sent_at {
            sent_at + self.pong_timeout
        } else {
            self.last_inbound_at + self.idle_threshold
        }
    }

    /// Whether a ping is currently outstanding. Diagnostic-only.
    #[cfg(test)]
    pub(crate) fn ping_in_flight(&self) -> bool {
        self.ping_sent_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    fn fresh() -> (Instant, KeepaliveState) {
        let t0 = Instant::now();
        (t0, KeepaliveState::new(t0, s(30), s(30)))
    }

    #[test]
    fn quiet_socket_emits_ping_after_idle_threshold() {
        let (t0, mut k) = fresh();
        // Just before threshold — nothing.
        assert_eq!(k.step(t0 + s(29)), KeepaliveAction::Idle);
        assert!(!k.ping_in_flight());
        // At threshold — emit.
        assert_eq!(k.step(t0 + s(30)), KeepaliveAction::EmitPing);
        assert!(k.ping_in_flight());
    }

    #[test]
    fn inbound_resets_idle_clock() {
        let (t0, mut k) = fresh();
        k.on_inbound(t0 + s(25));
        // 30s after t0 is only 5s since last inbound — still quiet.
        assert_eq!(k.step(t0 + s(30)), KeepaliveAction::Idle);
        // 55s after t0 is 30s since last inbound — emit.
        assert_eq!(k.step(t0 + s(55)), KeepaliveAction::EmitPing);
    }

    #[test]
    fn ping_in_flight_does_not_re_emit() {
        let (t0, mut k) = fresh();
        assert_eq!(k.step(t0 + s(30)), KeepaliveAction::EmitPing);
        // Five seconds later, still waiting for pong — idle, not another ping.
        assert_eq!(k.step(t0 + s(35)), KeepaliveAction::Idle);
        assert!(k.ping_in_flight());
    }

    #[test]
    fn pong_clears_in_flight_and_resets_idle() {
        let (t0, mut k) = fresh();
        assert_eq!(k.step(t0 + s(30)), KeepaliveAction::EmitPing);
        // Pong arrives 5s later — `on_inbound` is the signal.
        k.on_inbound(t0 + s(35));
        assert!(!k.ping_in_flight());
        // Next 30s of silence trigger a fresh ping.
        assert_eq!(k.step(t0 + s(64)), KeepaliveAction::Idle);
        assert_eq!(k.step(t0 + s(65)), KeepaliveAction::EmitPing);
    }

    #[test]
    fn pong_timeout_marks_dead() {
        let (t0, mut k) = fresh();
        assert_eq!(k.step(t0 + s(30)), KeepaliveAction::EmitPing);
        // 30s after ping with no inbound — dead.
        assert_eq!(k.step(t0 + s(60)), KeepaliveAction::Dead);
    }

    #[test]
    fn inbound_just_before_pong_timeout_saves_socket() {
        let (t0, mut k) = fresh();
        assert_eq!(k.step(t0 + s(30)), KeepaliveAction::EmitPing);
        // Pong arrives at the very edge of the window.
        k.on_inbound(t0 + s(59));
        assert_eq!(k.step(t0 + s(60)), KeepaliveAction::Idle);
        assert!(!k.ping_in_flight());
    }

    #[test]
    fn any_inbound_frame_counts_as_keepalive_signal() {
        // The relay sending a `["NOTICE", ...]` (or any other frame) counts
        // as "alive" — we don't strictly require a Pong. This matches the
        // "30s of inbound idleness" spec wording in T120b.
        let (t0, mut k) = fresh();
        assert_eq!(k.step(t0 + s(30)), KeepaliveAction::EmitPing);
        k.on_inbound(t0 + s(40)); // any frame, not necessarily Pong
        assert!(!k.ping_in_flight());
        assert_eq!(k.step(t0 + s(40)), KeepaliveAction::Idle);
    }

    #[test]
    fn dead_does_not_re_emit_ping_until_caller_handles_it() {
        // After Dead, the FSM stays in PingPending — caller is expected to
        // tear down. We don't auto-recover (that's reconnect's job).
        let (t0, mut k) = fresh();
        assert_eq!(k.step(t0 + s(30)), KeepaliveAction::EmitPing);
        assert_eq!(k.step(t0 + s(60)), KeepaliveAction::Dead);
        // Subsequent step is still Dead (or at least non-EmitPing) — caller
        // must reset by destroying this state on reconnect.
        assert_eq!(k.step(t0 + s(90)), KeepaliveAction::Dead);
    }

    #[test]
    fn boundary_at_exact_threshold_is_inclusive() {
        // The doc-spec says "every 30s of inbound idleness, emit a Ping".
        // We treat the equality case as the firing edge.
        let (t0, mut k) = fresh();
        assert_eq!(k.step(t0 + s(30)), KeepaliveAction::EmitPing);
    }
}
