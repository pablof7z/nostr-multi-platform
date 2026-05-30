//! V-87 D1 offline-first startup ordering tests.
//!
//! Two properties must hold for D1 / offline-first.md §3:
//!
//! 1. **Pre-command frame** (`#601`) — the actor emits at least one snapshot
//!    BEFORE the host sends any command.  A host that waits for the first frame
//!    before calling `Start` must not deadlock.
//!
//! 2. **Zero-relay startup** (`#602`) — a `Start` with zero relays connected
//!    must not block indefinitely and must emit at least one running snapshot.
//!    `maybe_send_startup` must not be gated on `all_relays_connected`.

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{run_actor, ActorCommand};
    use crate::update_envelope::{decode_update_frame, UpdateEnvelope};
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    // ─── helper ─────────────────────────────────────────────────────────────

    fn spawn_actor() -> (
        mpsc::Sender<ActorCommand>,
        mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
    ) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
        let (upd_tx, upd_rx) = mpsc::channel::<crate::update_envelope::UpdateFrameBytes>();
        let actor_self_tx = cmd_tx.clone();
        thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));
        (cmd_tx, upd_rx)
    }

    /// Drain all frames currently in `upd_rx` and decode them to
    /// `UpdateEnvelope::Snapshot` values.  Returns all snapshots received.
    fn drain_snapshots(
        upd_rx: &mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
        timeout: Duration,
    ) -> Vec<serde_json::Value> {
        let deadline = std::time::Instant::now() + timeout;
        let mut snapshots = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match upd_rx.recv_timeout(remaining) {
                Ok(frame) => {
                    if let Ok(UpdateEnvelope::Snapshot(v)) = decode_update_frame(&frame) {
                        snapshots.push(v);
                    }
                }
                Err(_) => break,
            }
        }
        snapshots
    }

    // ─── V-87 #601: pre-command frame ────────────────────────────────────────

    /// The actor MUST emit a snapshot before the host sends any command.
    ///
    /// Offline-first.md §3: "the first snapshot is unconditional … even if the
    /// working set is empty." A host that waits for the first frame before
    /// sending `Start` must not deadlock.
    ///
    /// Assertion: spawn the actor, do NOT send Start, and assert that at least
    /// one snapshot arrives on the update channel within a 500 ms window.
    #[test]
    fn v87_601_first_snapshot_arrives_before_start_command() {
        let (_cmd_tx, upd_rx) = spawn_actor();

        // Do NOT send Start or any other command.
        // The actor MUST emit a pre-flight frame independently.
        let snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));

        assert!(
            !snapshots.is_empty(),
            "V-87 #601: actor must emit at least one snapshot before \
             the host sends any command; offline-first.md §3 requires the \
             first snapshot to be unconditional (got 0 frames in 500 ms)"
        );
        // The pre-flight frame is emitted with `running=false`; the snapshot's
        // `running` field should be `false` (or absent).
        let first = &snapshots[0];
        assert!(
            first["running"].is_null() || first["running"] == serde_json::json!(false),
            "V-87 #601: pre-flight snapshot must carry running=false, \
             got running={:?}",
            first["running"]
        );
    }

    // ─── V-87 #602: zero-relay startup does not hang ─────────────────────────

    /// A `Start` with zero relay connections must complete and emit a running
    /// snapshot within a bounded budget.
    ///
    /// Offline-first.md §3: "startup MUST NOT wait on a subscription response,
    /// an EOSE, or any relay handshake before emitting its first snapshot."
    ///
    /// Previously `maybe_send_startup` was gated on `all_relays_connected`.
    /// With the fix, `startup_requests()` (bootstrap interest registration) fires
    /// unconditionally when `running=true`, so the planner and snapshot emit path
    /// are not blocked by absent relay connections.
    #[test]
    fn v87_602_start_with_zero_relays_emits_running_snapshot() {
        let (cmd_tx, upd_rx) = spawn_actor();

        // Wait for the pre-flight frame so the actor has had time to build
        // the real kernel.
        let _ = drain_snapshots(&upd_rx, Duration::from_millis(200));

        // Send Start — zero relays are configured (no `AddRelay` beforehand).
        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: DEFAULT_VISIBLE_LIMIT,
                emit_hz: 30,
            })
            .expect("send Start");

        // The actor must emit a snapshot with `running=true` within 500 ms
        // without needing any relay to connect first.
        let snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));

        let running_snapshot = snapshots
            .iter()
            .find(|s| s["running"] == serde_json::json!(true));

        assert!(
            running_snapshot.is_some(),
            "V-87 #602: Start with zero relays must produce a running=true \
             snapshot within 500 ms; maybe_send_startup must not be gated on \
             all_relays_connected (got {} total snapshots, none with running=true)",
            snapshots.len()
        );

        // Graceful shutdown.
        let _ = cmd_tx.send(ActorCommand::Shutdown);
    }

    // ─── V-87 combined: no deadlock on snapshot-first host ───────────────────

    /// A host that waits for a snapshot before sending `Start` must not
    /// deadlock — and must receive a running snapshot after sending `Start`.
    ///
    /// This is the end-to-end ordering property: pre-flight frame unblocks the
    /// host; the host then sends Start; the actor produces a running snapshot.
    #[test]
    fn v87_snapshot_first_host_no_deadlock() {
        let (cmd_tx, upd_rx) = spawn_actor();

        // Step 1: wait for the unconditional pre-flight frame.
        let pre_snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));
        assert!(
            !pre_snapshots.is_empty(),
            "V-87: no pre-flight snapshot arrived within 500 ms — would deadlock \
             a snapshot-first host"
        );

        // Step 2: now the host sends Start (as it would in production after
        // observing the first frame).
        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: DEFAULT_VISIBLE_LIMIT,
                emit_hz: 30,
            })
            .expect("send Start after pre-flight");

        // Step 3: a running=true snapshot must arrive within 500 ms.
        let post_snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));
        let running = post_snapshots
            .iter()
            .any(|s| s["running"] == serde_json::json!(true));
        assert!(
            running,
            "V-87: after Start no running=true snapshot arrived within 500 ms \
             (got {} post-Start snapshots)",
            post_snapshots.len()
        );

        let _ = cmd_tx.send(ActorCommand::Shutdown);
    }
}
