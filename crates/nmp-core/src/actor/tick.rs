//! Idle-tick timing helpers — `compute_wait`, `emit_now`, `flush_due`, and
//! the `emit_interval` utility.  Separated from the main loop so that the D8
//! invariant ("emit only when state changed") is concentrated in one file.

use crate::kernel::Kernel;
use crate::update_envelope::UpdateFrameBytes;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// D8 — hard ceiling on the snapshot-emit rate.
///
/// A host MAY request any `emit_hz` value but the actor MUST NOT spin faster
/// than 60 Hz.  Above this rate the snapshot serialisation cost begins to
/// measurably compete with relay ingest on the actor thread, and the
/// additional frames carry no useful new state for any 60-fps display.
///
/// Any `emit_hz` above this value is silently clamped to `EMIT_HZ_MAX` by
/// [`clamp_emit_hz`] (called in the `Start` and `Configure` dispatch arms).
/// A kernel log line is emitted so the violation is observable in diagnostics
/// (D6: no panics at configuration time).
pub const EMIT_HZ_MAX: u32 = 60;

/// Clamp a host-supplied `emit_hz` to the D8 ceiling.
///
/// Returns `(clamped_hz, was_clamped)`.  Callers use the boolean to decide
/// whether to emit a kernel log line.  The returned value is always in
/// `[1, EMIT_HZ_MAX]` — the lower bound comes from [`emit_interval`]'s
/// existing `hz.max(1)` guard; the upper bound is the new D8 gate.
#[inline]
pub(super) fn clamp_emit_hz(hz: u32) -> (u32, bool) {
    if hz > EMIT_HZ_MAX {
        (EMIT_HZ_MAX, true)
    } else {
        (hz, false)
    }
}

/// Clamp a host-supplied `emit_hz` to the D8 ceiling, emitting a kernel log
/// line (D6: observable, not a panic) when a clamp occurs.
///
/// `site` names the dispatch arm (`"Start"` / `"Configure"`) for the log line.
/// Returns the clamped value to store in the actor's `emit_hz`. This is the
/// single entry point the `Start` and `Configure` arms call so the clamp +
/// logging policy lives in one place.
pub(super) fn clamp_emit_hz_logged(kernel: &mut Kernel, requested_hz: u32, site: &str) -> u32 {
    let (hz, was_clamped) = clamp_emit_hz(requested_hz);
    if was_clamped {
        kernel.log(format!(
            "D8: {site} emit_hz={requested_hz} exceeds the {EMIT_HZ_MAX} Hz \
             ceiling — clamped to {hz} Hz"
        ));
    }
    hz
}

/// Compute how long the actor loop should block on `relay_rx.recv_timeout`.
///
/// When the kernel has un-emitted changes and we are running, returns the
/// time remaining until the next emit window (clamped to zero). Otherwise
/// returns 250 ms so that time-gated kernel gates (e.g. `contacts_deadline`)
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
    Duration::from_secs_f64(1.0 / f64::from(emit_hz.max(1)))
}

pub(super) fn flush_due(kernel: &Kernel, running: bool, last_emit: Instant, emit_hz: u32) -> bool {
    running && kernel.changed_since_emit() && last_emit.elapsed() >= emit_interval(emit_hz)
}

pub(super) fn emit_now(
    kernel: &mut Kernel,
    running: bool,
    update_tx: &Sender<UpdateFrameBytes>,
    last_emit: &mut Instant,
) {
    let _ = update_tx.send(kernel.make_update(running));
    *last_emit = Instant::now();
}

/// T114b — post-dispatch emit gate (per-dispatch retention audit).
///
/// View-command dispatchers (`ClaimProfile`, … — everything in
/// `dispatch.rs` that mutates kernel state but is NOT a lifecycle event) MUST
/// route through this helper. It emits the snapshot only when `running=true`,
/// matching the idle-tick path's gating contract (see [`compute_wait`]).
///
/// When the kernel is in `running=false` state (the harness Configure-not-Start
/// pattern used by S1–S5, and the `nmp_app_configure` mid-process call before
/// any `Start`) there is no UI consumer subscribed to the snapshot channel.
/// Per-dispatch emits in that mode (a) produce no useful work (the listener
/// fires `sink_cb` with no consumer) and (b) push binary frames onto the
/// unbounded kernel→listener mpsc whose internal block free-list retains
/// segments long after the frames themselves are dropped — the dominant
/// per-dispatch retention source measured in `s2-drain-analysis.md`.
///
/// Lifecycle commands (`Start`, `Configure`, `Reset`, `Stop`, `Shutdown`) MUST
/// keep using [`emit_now`] directly — they need to deliver an initial /
/// terminal snapshot regardless of the running flag.
///
/// When `running=true`, behavior is identical to [`emit_now`] (immediate
/// snapshot delivery). The bottom-of-main-loop `flush_due` gate already
/// enforces `emit_hz` rate-limit for state that changes faster than the UI
/// can consume — this helper does not duplicate that.
pub(super) fn maybe_emit_after_dispatch(
    kernel: &mut Kernel,
    running: bool,
    update_tx: &Sender<UpdateFrameBytes>,
    last_emit: &mut Instant,
) {
    if running {
        emit_now(kernel, running, update_tx, last_emit);
    }
    // When !running, state changes (e.g. claim_profile updating
    // profile_claims) remain visible through `changed_since_emit`; the next
    // `Start` command's `emit_now` will deliver the up-to-date snapshot.
}

// ── D8 regression test ───────────────────────────────────────────────────────

// D8 emit_hz ceiling tests — kept beside `tick.rs` (off the ratcheted
// `actor/mod.rs` module list) so this PR does not touch that file.
#[cfg(all(test, feature = "native"))]
mod emit_hz_clamp_tests;

// ADR-0050 §D3a / issue #1231 — drives the real `run_actor` loop to lock the
// single-waking-inbox wake property AND the single `drain_command_lane` routing
// (follow-up #3). Kept here (sibling module, off `actor/mod.rs`'s ratcheted
// list and out of `mod tests` to avoid collisions).
#[cfg(all(test, feature = "native"))]
mod command_wakes_blocked_actor_tests;

#[cfg(test)]
mod tests {
    use crate::actor::{run_actor, ActorCommand, ActorMail, CommandSender};
    use crate::app::KernelAction;
    use crate::kernel::Kernel;
    use crate::transport::wire as fb;
    use crate::update_envelope::{
        decode_snapshot_envelope, decode_snapshot_typed_projections, UpdateFrameBytes,
    };
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    /// PR-B (#991/#979): decode a frame as a snapshot envelope (Tier-3 typed fields).
    /// Returns `Some(envelope)` for snapshot frames; panics on malformed input;
    /// returns `None` for panic frames (so callers can test for unexpected panics).
    fn decode_as_snapshot(frame: &[u8]) -> Option<crate::update_envelope::SnapshotEnvelope> {
        assert!(
            fb::update_frame_buffer_has_identifier(frame),
            "frame must carry the NMPU identifier"
        );
        let root = fb::root_as_update_frame(frame).expect("valid UpdateFrame");
        match root.kind() {
            k if k == fb::FrameKind::Panic => None,
            k if k == fb::FrameKind::Snapshot => {
                Some(decode_snapshot_envelope(frame).expect("snapshot envelope decode"))
            }
            other => panic!("unknown frame kind {}", other.0),
        }
    }

    /// Verifies that idle ticks do not emit snapshots when kernel state has not
    /// changed (D8: zero false-wakeup allocations after warmup — codex T23 P2).
    ///
    /// V-87 #601 (offline-first §3): the actor now emits exactly ONE pre-flight
    /// snapshot immediately on startup, before the host sends any command, so
    /// that a host waiting for the first frame before sending `Start` does not
    /// deadlock.  That pre-flight frame is the ONLY snapshot expected here.
    /// Over the subsequent 1 s the 250 ms idle-poll fires ~4 more times; none
    /// should produce additional snapshots (state has not changed).
    #[test]
    fn idle_ticks_do_not_emit_snapshots_when_state_unchanged() {
        let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
        let cmd_tx = CommandSender::new(inbox_tx);
        let (upd_tx, upd_rx) = mpsc::channel::<UpdateFrameBytes>();
        let actor_self_tx = cmd_tx.clone();
        thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));

        // Wait long enough for several idle-poll cycles without any commands.
        thread::sleep(Duration::from_millis(1_000));

        let _ = cmd_tx.send(ActorCommand::Shutdown);

        let mut idle_count = 0_usize;
        while upd_rx.try_recv().is_ok() {
            idle_count += 1;
        }

        // Exactly 1 is expected: the D1 pre-flight frame (offline-first §3).
        // More than 1 is the D8 false-wakeup regression.
        assert_eq!(
            idle_count, 1,
            "D8 regression: actor emitted {idle_count} snapshot(s) without any \
             Start command or state change; expected exactly 1 (the V-87 D1 \
             pre-flight frame only — no additional spurious frames)"
        );
    }

    /// End-to-end: a live actor emits decodable frames on the single channel,
    /// and every frame decodes as exactly one `UpdateEnvelope` (the canonical
    /// T103 contract). `Start` yields a snapshot frame; `Kernel(OpenView)`
    /// no longer emits a discrete frame (WireDelta was deleted as
    /// shipped-but-inert), but the periodic snapshot still flows.
    #[test]
    fn live_actor_frames_are_all_decodable_envelopes() {
        let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
        let cmd_tx = CommandSender::new(inbox_tx);
        let (upd_tx, upd_rx) = mpsc::channel::<UpdateFrameBytes>();
        let actor_self_tx = cmd_tx.clone();
        thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));

        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: 50,
                emit_hz: 30,
                initial_relays: Vec::new(),
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

        let mut snapshots = 0usize;
        while let Ok(frame) = upd_rx.try_recv() {
            // Every frame MUST decode as a valid typed-envelope (PR-B: payload
            // zeroed, so we use the Tier-3 typed decoder instead of the JSON one).
            match decode_as_snapshot(&frame) {
                Some(envelope) => {
                    // Every snapshot MUST carry a non-zero kernel_schema_version
                    // so a shell can detect a mismatch and degrade (D1). The
                    // `SNAPSHOT_SCHEMA_VERSION` constant is 1; check the Tier-3
                    // typed field (the source of truth after PR-B).
                    assert_eq!(
                        envelope.kernel_schema_version,
                        crate::update_envelope::SNAPSHOT_SCHEMA_VERSION,
                        "snapshot frame must carry kernel_schema_version=1"
                    );
                    snapshots += 1;
                }
                // D7 — no panic is induced in this happy-path test; a panic
                // frame here would be an actor-death regression.
                None => panic!("unexpected actor-death (Panic) frame on the channel"),
            }
        }

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
        let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
        let cmd_tx = CommandSender::new(inbox_tx);
        let (upd_tx, upd_rx) = mpsc::channel::<UpdateFrameBytes>();
        let actor_self_tx = cmd_tx.clone();
        thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));

        // Configure (NOT Start) — running stays false. Then fire a flurry of
        // view commands. None of these should produce a snapshot frame.
        cmd_tx
            .send(ActorCommand::Configure {
                visible_limit: 50,
                emit_hz: 30,
            })
            .unwrap();
        let pk = "0".repeat(64);
        // V-68 / V-112 (ADR-0042): OpenAuthor / CloseAuthor deleted.
        // Dispatch claim/release_profile to flood the queue (same fire-and-forget path).
        for i in 0..50u64 {
            cmd_tx
                .send(ActorCommand::ClaimProfile {
                    pubkey: pk.clone(),
                    consumer_id: format!("test-consumer-{i}"),
                    force: false,
                })
                .unwrap();
            cmd_tx
                .send(ActorCommand::ReleaseProfile {
                    pubkey: pk.clone(),
                    consumer_id: format!("test-consumer-{i}"),
                })
                .unwrap();
        }
        // The actor may be inside the 250 ms idle relay wait before it
        // checks the command channel, so wait past one full idle cycle.
        thread::sleep(Duration::from_millis(350));
        let _ = cmd_tx.send(ActorCommand::Shutdown);

        let mut snapshots = 0usize;
        while let Ok(frame) = upd_rx.try_recv() {
            // PR-B: decode via Tier-3 envelope (payload zeroed).
            if !fb::update_frame_buffer_has_identifier(&frame) {
                continue; // malformed — skip
            }
            if let Ok(root) = fb::root_as_update_frame(&frame) {
                match root.kind() {
                    k if k == fb::FrameKind::Snapshot => snapshots += 1,
                    k if k == fb::FrameKind::Panic => {
                        let msg = root.panic().map(|p| p.msg().to_string()).unwrap_or_default();
                        panic!("unexpected actor-death frame on the channel: {msg}")
                    }
                    _ => {}
                }
            }
        }

        // V-87 #601 — the actor emits one pre-flight frame (D1 / offline-first
        // §3) unconditionally at startup.  Configure ITSELF also emits one
        // snapshot (the lifecycle event).  So the expected upper bound is now
        // 2 (pre-flight + Configure).  View dispatches must not add to the
        // count — the S2 retention constraint is that per-dispatch emits while
        // `running=false` are suppressed; that invariant is unchanged.
        assert!(
            snapshots <= 2,
            "regression: view-command dispatches emitted {snapshots} snapshot(s) \
             while running=false; expected ≤ 2 (V-87 pre-flight + Configure only). \
             This is the S2 retention leak — see s2-retention-audit.md."
        );
    }

    /// T114b regression positive — when `running=true`, view-command dispatches
    /// MUST emit snapshots. Pins the other direction of the `running` gate so a
    /// future "optimization" doesn't drop emits entirely and break the UI.
    #[test]
    fn view_dispatches_emit_snapshots_when_running() {
        let (upd_tx, upd_rx) = mpsc::channel::<UpdateFrameBytes>();
        let mut kernel = Kernel::new(50);
        let mut last_emit = Instant::now();

        let pk = "0".repeat(64);
        let _ = kernel.claim_profile(pk, "test-consumer".into(), false, false);
        super::maybe_emit_after_dispatch(&mut kernel, true, &upd_tx, &mut last_emit);

        let frame = upd_rx
            .recv_timeout(Duration::from_millis(50))
            .expect("running=true view dispatch must emit a snapshot");
        // PR-B: use Tier-3 typed decoder (payload is no longer emitted on the wire).
        assert!(
            decode_as_snapshot(&frame).is_some(),
            "regression: running=true + view dispatch emitted a non-snapshot frame"
        );
    }

    /// Verify create_account emits a snapshot with activeAccount set.
    #[test]
    fn create_account_emits_snapshot_with_active_account() {
        let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
        let cmd_tx = CommandSender::new(inbox_tx);
        let (upd_tx, upd_rx) = mpsc::channel::<UpdateFrameBytes>();
        let actor_self_tx = cmd_tx.clone();
        thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));

        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: 50,
                emit_hz: 30,
                initial_relays: Vec::new(),
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
                mls: false,
                make_active: true,
            })
            .unwrap();

        // Wait for create_account to process and emit.
        thread::sleep(Duration::from_millis(500));
        let _ = cmd_tx.send(ActorCommand::Shutdown);

        // Drain all snapshots and find the one with active_account set.
        // PR-B (#991/#979): `active_account` is now a Tier-2 built-in typed
        // projection (KACT file identifier) in the `typed_projections` sidecar —
        // no longer in the JSON `payload` (which is zeroed). Decode the typed
        // sidecar and check for a non-None pubkey.
        let mut found_active = false;
        while let Ok(frame) = upd_rx.try_recv() {
            if decode_as_snapshot(&frame).is_none() {
                continue; // panic frame — already checked above
            }
            if let Ok(typed) = decode_snapshot_typed_projections(&frame) {
                let active_entry = typed
                    .iter()
                    .find(|p| p.key == crate::kernel::public_typed_projections::ACTIVE_ACCOUNT_SCHEMA_ID);
                if let Some(entry) = active_entry {
                    if let Ok(model) = crate::kernel::public_typed_projections::decode_active_account(&entry.payload) {
                        if model.pubkey.is_some() {
                            found_active = true;
                        }
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
