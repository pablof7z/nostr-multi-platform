//! D8 — emit_hz ceiling enforcement tests.
//!
//! Proves the `EMIT_HZ_MAX` (60 Hz) clamp in `actor/tick.rs`: pure
//! `clamp_emit_hz` boundary checks plus a live-actor end-to-end test that a
//! host-requested 10 kHz rate is clamped (the actor cannot spin faster than the
//! ceiling). Split out of `tick.rs` to keep that file within its LOC ceiling.

use super::{clamp_emit_hz, EMIT_HZ_MAX};
use crate::actor::{run_actor, ActorCommand, ActorMail, CommandSender};
use crate::update_envelope::UpdateFrameBytes;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// D8: `clamp_emit_hz` returns the input unchanged for values within the
/// ceiling (`≤ EMIT_HZ_MAX`).
#[test]
fn clamp_emit_hz_passthrough_at_ceiling() {
    let (hz, clamped) = clamp_emit_hz(EMIT_HZ_MAX);
    assert_eq!(hz, EMIT_HZ_MAX);
    assert!(!clamped, "value equal to ceiling must not be flagged as clamped");

    let (hz_low, clamped_low) = clamp_emit_hz(4);
    assert_eq!(hz_low, 4);
    assert!(!clamped_low);
}

/// D8: `clamp_emit_hz` enforces the ceiling for out-of-range values and signals
/// the violation so the caller can emit a log line.
#[test]
fn clamp_emit_hz_enforces_ceiling() {
    let (hz, clamped) = clamp_emit_hz(10_000);
    assert_eq!(
        hz, EMIT_HZ_MAX,
        "10 000 Hz must be clamped to EMIT_HZ_MAX ({EMIT_HZ_MAX})"
    );
    assert!(clamped, "clamping must be reported");

    let (hz2, clamped2) = clamp_emit_hz(EMIT_HZ_MAX + 1);
    assert_eq!(hz2, EMIT_HZ_MAX);
    assert!(clamped2);
}

/// D8 end-to-end: the actor started with `emit_hz = 10_000` MUST NOT emit faster
/// than the `EMIT_HZ_MAX` (60 Hz) ceiling.
///
/// Implementation note: at the unclamped 10 kHz the actor would emit ~2 000
/// frames in the 200 ms window; at the clamped 60 Hz ceiling it can emit at most
/// ~12. We assert ≤ a 3×-margin bound — generous for CI jitter while still
/// catching a regression that removes the clamp.
#[test]
fn high_emit_hz_is_clamped_to_ceiling_end_to_end() {
    let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
    let cmd_tx = CommandSender::new(inbox_tx);
    let (upd_tx, upd_rx) = mpsc::channel::<UpdateFrameBytes>();
    let actor_self_tx = cmd_tx.clone();
    thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));

    // Request an absurdly high rate (100× the ceiling).
    cmd_tx
        .send(ActorCommand::Start {
            visible_limit: 50,
            emit_hz: 10_000,
            initial_relays: Vec::new(),
        })
        .unwrap();

    thread::sleep(Duration::from_millis(200));
    let _ = cmd_tx.send(ActorCommand::Shutdown);

    let mut frame_count = 0usize;
    while upd_rx.try_recv().is_ok() {
        frame_count += 1;
    }

    // 1/EMIT_HZ_MAX * 200 ms ≈ 12 frames maximum. Allow 3× margin for jitter and
    // the pre-flight frame.
    let max_expected = (EMIT_HZ_MAX as usize) * 200 / 1_000 * 3 + 1;
    assert!(
        frame_count <= max_expected,
        "D8 regression: actor emitted {frame_count} frames in 200 ms with \
         emit_hz=10_000; expected ≤ {max_expected} (ceiling clamped to \
         {EMIT_HZ_MAX} Hz). The clamp in `clamp_emit_hz` may have been removed."
    );
}
