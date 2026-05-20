//! Idle-tick timing helpers — `compute_wait`, `emit_now`, `flush_due`, and
//! the `emit_interval` utility.  Separated from the main loop so that the D8
//! invariant ("emit only when state changed") is concentrated in one file.

use crate::app::KernelUpdate;
use crate::kernel::Kernel;
use crate::update_envelope::{wrap_snapshot, wrap_update};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// Compute how long the actor loop should block on `relay_rx.recv_timeout`.
///
/// When the kernel has un-emitted changes and we are running, returns the
/// time remaining until the next emit window (clamped to zero). Otherwise
/// returns 250 ms so that time-gated kernel gates (e.g. contacts_deadline)
/// are checked at a reasonable cadence even with no relay traffic.
pub(super) fn compute_wait(
    kernel: &Kernel,
    running: bool,
    last_emit: Instant,
    emit_hz: u32,
) -> Duration {
    let wait = if running && kernel.changed_since_emit() {
        emit_interval(emit_hz)
            .checked_sub(last_emit.elapsed())
            .unwrap_or(Duration::ZERO)
    } else {
        Duration::from_millis(250)
    };
    // Prevent busy-waiting if emit_hz is accidentally very high.
    wait.max(Duration::from_millis(1))
}

pub(super) fn emit_interval(emit_hz: u32) -> Duration {
    Duration::from_secs_f64(1.0 / emit_hz.max(1) as f64)
}

pub(super) fn flush_due(kernel: &Kernel, running: bool, last_emit: Instant, emit_hz: u32) -> bool {
    running && kernel.changed_since_emit() && last_emit.elapsed() >= emit_interval(emit_hz)
}

pub(super) fn emit_now(
    kernel: &mut Kernel,
    running: bool,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
) {
    // Snapshot frame: wrap the already-serialized snapshot as
    // `{"t":"snapshot","v":…}` (D8 — borrowed RawValue, one outer alloc, no
    // re-parse). D6 — a wrap failure drops the frame, never unwinds.
    if let Some(frame) = wrap_snapshot(kernel.make_update(running)) {
        let _ = update_tx.send(frame);
    }
    *last_emit = Instant::now();
}

/// T114b — post-dispatch emit gate (per-dispatch retention audit).
///
/// View-command dispatchers (`OpenAuthor`, `ClaimProfile`, … — everything in
/// `dispatch.rs` that mutates kernel state but is NOT a lifecycle event) MUST
/// route through this helper. It emits the snapshot only when `running=true`,
/// matching the idle-tick path's gating contract (see [`compute_wait`]).
///
/// When the kernel is in `running=false` state (the harness Configure-not-Start
/// pattern used by S1–S5, and the `nmp_app_configure` mid-process call before
/// any `Start`) there is no UI consumer subscribed to the snapshot channel.
/// Per-dispatch emits in that mode (a) produce no useful work (the listener
/// fires `sink_cb` with no consumer) and (b) push `String` frames onto the
/// unbounded kernel→listener mpsc whose internal block free-list retains
/// segments long after the strings themselves are dropped — the dominant
/// per-dispatch retention source measured in `s2-drain-analysis.md`.
///
/// Lifecycle commands (`Start`, `Configure`, `Reset`, `Stop`, `Shutdown`) MUST
/// keep using [`emit_now`] directly — they need to deliver an initial /
/// terminal snapshot regardless of the running flag.
///
/// When `running=true`, behavior is identical to [`emit_now`] (immediate
/// snapshot delivery). The bottom-of-main-loop `flush_due` gate already
/// enforces emit_hz rate-limit for state that changes faster than the UI
/// can consume — this helper does not duplicate that.
pub(super) fn maybe_emit_after_dispatch(
    kernel: &mut Kernel,
    running: bool,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
) {
    if running {
        emit_now(kernel, running, update_tx, last_emit);
    }
    // When !running, state changes (e.g. claim_profile updating
    // profile_claims) remain visible through `changed_since_emit`; the next
    // `Start` command's `emit_now` will deliver the up-to-date snapshot.
}

/// Push a discrete [`KernelUpdate`] onto the channel as the tagged
/// `{"t":"update","v":…}` frame so consumers decode the **one**
/// [`crate::UpdateEnvelope`] type (D6 — the tag is the discriminant).
pub(super) fn emit_kernel_update(update: &KernelUpdate, update_tx: &Sender<String>) {
    if let Some(frame) = wrap_update(update) {
        let _ = update_tx.send(frame);
    }
}

// ── D8 regression test ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::actor::{run_actor, ActorCommand};
    use crate::app::KernelAction;
    use crate::kernel::Kernel;
    use crate::update_envelope::UpdateEnvelope;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    /// Verifies that idle ticks do not emit snapshots when kernel state has not
    /// changed (D8: zero false-wakeup allocations after warmup — codex T23 P2).
    ///
    /// The actor is spawned WITHOUT sending `Start`, so no relays connect and
    /// `changed_since_emit` never becomes true.  Over 1 s the 250 ms idle-poll
    /// fires ~4 times; none should produce a snapshot.
    #[test]
    fn idle_ticks_do_not_emit_snapshots_when_state_unchanged() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
        let (upd_tx, upd_rx) = mpsc::channel::<String>();
        thread::spawn(move || run_actor(cmd_rx, upd_tx));

        // Wait long enough for several idle-poll cycles without any commands.
        thread::sleep(Duration::from_millis(1_000));

        let _ = cmd_tx.send(ActorCommand::Shutdown);

        let mut idle_count = 0_usize;
        while upd_rx.try_recv().is_ok() {
            idle_count += 1;
        }

        assert_eq!(
            idle_count, 0,
            "D8 regression: actor emitted {idle_count} snapshot(s) without any \
             Start command or state change; expected 0"
        );
    }

    /// End-to-end: a live actor emits BOTH wire shapes on the single channel,
    /// and every frame decodes as exactly one `UpdateEnvelope` (the canonical
    /// T103 contract). `Start` yields a snapshot frame; `Kernel(OpenView)`
    /// yields a discrete update frame followed by a snapshot frame.
    #[test]
    fn live_actor_frames_are_all_decodable_envelopes() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
        let (upd_tx, upd_rx) = mpsc::channel::<String>();
        thread::spawn(move || run_actor(cmd_rx, upd_tx));

        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: 50,
                emit_hz: 30,
            })
            .unwrap();
        cmd_tx
            .send(ActorCommand::Kernel(KernelAction::OpenView {
                namespace: "profile".into(),
                key: "pk".into(),
            }))
            .unwrap();

        // Let the actor process both commands and flush.
        thread::sleep(Duration::from_millis(300));
        let _ = cmd_tx.send(ActorCommand::Shutdown);

        let mut updates = 0usize;
        let mut snapshots = 0usize;
        while let Ok(frame) = upd_rx.try_recv() {
            // Every frame MUST decode as the single discriminated type — this
            // is exactly what each host does.
            match serde_json::from_str::<UpdateEnvelope>(&frame)
                .unwrap_or_else(|e| panic!("undecodable frame on channel: {e}: {frame}"))
            {
                UpdateEnvelope::Update(_) => updates += 1,
                UpdateEnvelope::Snapshot(v) => {
                    // Every snapshot MUST carry a schema version so a shell can
                    // detect a kernel-vs-shell mismatch and degrade (D1). This
                    // pins the contract as a CI gate — removing the field can
                    // no longer slip past `serde_json::Value`'s tolerance.
                    assert_eq!(
                        v["schema_version"],
                        serde_json::json!(1),
                        "snapshot frame must carry schema_version=1: {v}"
                    );
                    snapshots += 1;
                }
                // D7 — no panic is induced in this happy-path test; a panic
                // frame here would be an actor-death regression.
                UpdateEnvelope::Panic(p) => {
                    panic!("unexpected actor-death frame on the channel: {}", p.msg)
                }
            }
        }

        assert!(
            updates >= 1,
            "expected ≥1 discrete update frame from Kernel(OpenView); got {updates}"
        );
        assert!(
            snapshots >= 1,
            "expected ≥1 snapshot frame from Start/emit; got {snapshots}"
        );
    }

    /// T114b regression — view-command dispatches (no preceding `Start`) MUST
    /// NOT emit snapshots. The S2 dispatch-flood scenario configures the
    /// actor without starting it; an emit-per-dispatch in that mode is the
    /// dominant per-dispatch retention source (see `s2-drain-analysis.md`).
    /// This pins `maybe_emit_after_dispatch`'s `running` gate so the leak
    /// cannot regress.
    #[test]
    fn view_dispatches_do_not_emit_snapshots_when_not_running() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
        let (upd_tx, upd_rx) = mpsc::channel::<String>();
        thread::spawn(move || run_actor(cmd_rx, upd_tx));

        // Configure (NOT Start) — running stays false. Then fire a flurry of
        // view commands. None of these should produce a snapshot frame; the
        // discrete-update frames (Kernel actions) are emitted regardless and
        // remain countable below.
        cmd_tx
            .send(ActorCommand::Configure {
                visible_limit: 50,
                emit_hz: 30,
            })
            .unwrap();
        let pk = "0".repeat(64);
        for _ in 0..50 {
            cmd_tx
                .send(ActorCommand::ClaimProfile {
                    pubkey: pk.clone(),
                    consumer_id: "test-consumer".into(),
                })
                .unwrap();
            cmd_tx
                .send(ActorCommand::OpenAuthor { pubkey: pk.clone() })
                .unwrap();
            cmd_tx
                .send(ActorCommand::CloseAuthor { pubkey: pk.clone() })
                .unwrap();
        }
        // The actor may be inside the 250 ms idle relay wait before it
        // checks the command channel, so wait past one full idle cycle.
        thread::sleep(Duration::from_millis(350));
        let _ = cmd_tx.send(ActorCommand::Shutdown);

        let mut snapshots = 0usize;
        let mut updates = 0usize;
        while let Ok(frame) = upd_rx.try_recv() {
            match serde_json::from_str::<UpdateEnvelope>(&frame) {
                Ok(UpdateEnvelope::Snapshot(_)) => snapshots += 1,
                Ok(UpdateEnvelope::Update(_)) => updates += 1,
                Ok(UpdateEnvelope::Panic(p)) => {
                    panic!("unexpected actor-death frame on the channel: {}", p.msg)
                }
                Err(_) => {} // ignore: legacy untagged frames
            }
        }

        // Configure ITSELF emits one snapshot — that's the lifecycle event,
        // which is allowed. View dispatches must not add to the count.
        assert!(
            snapshots <= 1,
            "regression: view-command dispatches emitted {snapshots} snapshot(s) \
             while running=false; expected ≤ 1 (lifecycle Configure only). \
             This is the S2 retention leak — see s2-retention-audit.md."
        );
        // No Kernel actions sent → no discrete updates expected.
        assert_eq!(
            updates, 0,
            "expected 0 discrete-update frames; got {updates}"
        );
    }

    /// T114b regression positive — when `running=true`, view-command dispatches
    /// MUST emit snapshots. Pins the other direction of the `running` gate so a
    /// future "optimization" doesn't drop emits entirely and break the UI.
    #[test]
    fn view_dispatches_emit_snapshots_when_running() {
        let (upd_tx, upd_rx) = mpsc::channel::<String>();
        let mut kernel = Kernel::new(50);
        let mut last_emit = Instant::now();

        let pk = "0".repeat(64);
        let _ = kernel.claim_profile(pk, "test-consumer".into(), false);
        super::maybe_emit_after_dispatch(&mut kernel, true, &upd_tx, &mut last_emit);

        let frame = upd_rx
            .recv_timeout(Duration::from_millis(50))
            .expect("running=true view dispatch must emit a snapshot");
        assert!(
            matches!(
                serde_json::from_str::<UpdateEnvelope>(&frame),
                Ok(UpdateEnvelope::Snapshot(_))
            ),
            "regression: running=true + view dispatch emitted a non-snapshot frame: {frame}"
        );
    }

    /// Verify create_account emits a snapshot with activeAccount set.
    #[test]
    fn create_account_emits_snapshot_with_active_account() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
        let (upd_tx, upd_rx) = mpsc::channel::<String>();
        thread::spawn(move || run_actor(cmd_rx, upd_tx));

        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: 50,
                emit_hz: 30,
            })
            .unwrap();

        // Wait for Start to process and emit initial snapshot.
        thread::sleep(Duration::from_millis(100));

        cmd_tx
            .send(ActorCommand::CreateAccount {
                profile: [("name".to_string(), "Test".to_string())]
                    .into_iter()
                    .collect(),
                relays: vec![("wss://relay.primal.net".to_string(), "both".to_string())],
            })
            .unwrap();

        // Wait for create_account to process and emit.
        thread::sleep(Duration::from_millis(500));
        let _ = cmd_tx.send(ActorCommand::Shutdown);

        // Drain all snapshots and find the one with activeAccount.
        let mut found_active = false;
        while let Ok(frame) = upd_rx.try_recv() {
            if let Ok(UpdateEnvelope::Snapshot(snap)) =
                serde_json::from_str::<UpdateEnvelope>(&frame)
            {
                if let Some(active) = snap.get("active_account") {
                    if !active.is_null() {
                        found_active = true;
                    }
                }
            }
        }
        assert!(
            found_active,
            "expected snapshot with activeAccount after CreateAccount"
        );
    }
}
