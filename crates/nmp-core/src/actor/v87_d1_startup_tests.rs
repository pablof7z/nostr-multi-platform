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
//!
//! 3. **Rev monotonicity** (`#601-rev`) — the real kernel's first
//!    `running=true` frame MUST carry a `rev` strictly greater than the
//!    pre-flight frame's `rev`, so the iOS host's
//!    `guard update.rev > rev` (KernelModel.swift:643) never silently drops
//!    it.  Without the `resume_rev_after_preflight` fix both frames carry
//!    `rev=1` and the host drops the `running=true` frame, leaving the UI
//!    stuck on the `running=false` pre-flight state indefinitely.

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
    /// **Rev-guard simulation (#601-rev)**: this test faithfully simulates the
    /// shipping iOS host's `guard update.rev > rev` guard
    /// (KernelModel.swift:643).  Frames are only "accepted" if their `rev` is
    /// strictly greater than the last-accepted rev, exactly as the host does.
    ///
    /// Without the `resume_rev_after_preflight` fix:
    /// - Pre-flight frame: `rev=1` → accepted (host had `rev=0`).
    /// - Start frame:      `rev=1` → REJECTED (`1 > 1` is false) → test fails.
    /// - Subsequent idle ticks: `changed_since_emit=false` → no further frames
    ///   → the host stays stuck on the `running=false` state indefinitely.
    ///
    /// With the fix:
    /// - Pre-flight frame: `rev=1` → accepted.
    /// - Start frame:      `rev=2` → accepted (`2 > 1`) → `running=true` → passes.
    #[test]
    fn v87_snapshot_first_host_no_deadlock() {
        let (cmd_tx, upd_rx) = spawn_actor();

        // ── Step 1: receive the unconditional pre-flight frame ───────────────
        let pre_snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));
        assert!(
            !pre_snapshots.is_empty(),
            "V-87: no pre-flight snapshot arrived within 500 ms — would deadlock \
             a snapshot-first host"
        );

        // Extract the pre-flight frame's rev.  The `rev` field MUST be present
        // and > 0; if it reads as null the host guard is a no-op and this test
        // would pass vacuously — catch that here.
        let preflight_rev = pre_snapshots[0]["rev"]
            .as_u64()
            .expect("V-87: pre-flight snapshot must carry a non-null numeric `rev` field; \
                     got null — the host guard simulation would be a no-op");
        assert!(
            preflight_rev > 0,
            "V-87: pre-flight rev must be ≥ 1 (got {preflight_rev})"
        );

        // ── Step 2: host sends Start after observing the pre-flight frame ────
        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: DEFAULT_VISIBLE_LIMIT,
                emit_hz: 30,
            })
            .expect("send Start after pre-flight");

        // ── Step 3: simulate the iOS host's `guard update.rev > rev` guard ───
        //
        // Collect post-Start frames and apply the same monotonicity filter the
        // shipping iOS host applies (KernelModel.swift:643).  Only frames with
        // `rev > last_accepted_rev` are "accepted"; the pre-flight frame already
        // moved `last_accepted_rev` to `preflight_rev`.
        let post_snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));

        // The FIRST post-Start frame MUST have rev strictly greater than
        // `preflight_rev`.  This is the key invariant: the Start dispatch's
        // `emit_now` produces the very first `running=true` frame; if that frame
        // has the same rev as the pre-flight frame the host drops it silently.
        // A host in an offline scenario with no subsequent relay events would
        // receive no further frames (changed_since_emit=false, no relay activity
        // to flip it back true), leaving the UI stuck on running=false forever.
        //
        // Without the fix: pre-flight=rev 1, Start frame=rev 1 → guard drops it.
        // With    the fix: pre-flight=rev 1, Start frame=rev 2 → guard accepts it.
        let first_post_start = post_snapshots.first().expect(
            "V-87 #601-rev: no post-Start frames received at all within 500 ms"
        );
        let first_post_start_rev = first_post_start["rev"]
            .as_u64()
            .expect("V-87 #601-rev: first post-Start snapshot missing `rev` field");

        assert!(
            first_post_start_rev > preflight_rev,
            "V-87 #601-rev: Start frame rev={first_post_start_rev} is NOT strictly \
             greater than pre-flight rev={preflight_rev}. \
             The iOS host's `guard update.rev > rev` (KernelModel.swift:643) would \
             silently drop this frame. In an offline scenario with no relay activity, \
             changed_since_emit stays false after the dropped Start emit and no \
             further frames are sent — the host is stuck on running=false indefinitely. \
             Fix: call kernel.resume_rev_after_preflight(preflight_rev) before the \
             dispatch loop so the real kernel's first make_update produces \
             rev = preflight_rev + 1."
        );

        // Belt-and-suspenders: also verify the first accepted frame carries running=true.
        assert_eq!(
            first_post_start["running"],
            serde_json::json!(true),
            "V-87 #601-rev: first post-Start frame has rev={first_post_start_rev} > \
             preflight_rev={preflight_rev} (guard passes) but running is not true: {:?}",
            first_post_start["running"]
        );

        let _ = cmd_tx.send(ActorCommand::Shutdown);
    }
}
