//! Integration test for the single waking actor inbox driven through the real
//! `run_actor` loop (ADR-0050 §D3a, issue #1231 follow-up #3).
//!
//! This is a NEW sibling test module (kept off `actor/mod.rs`'s ratcheted
//! module list and out of `tick.rs`'s `mod tests` block to avoid name
//! collisions) that locks two coupled properties end-to-end:
//!
//! 1. **§D3a wake property** — a command sent to an otherwise-idle actor that
//!    is blocked in the relay-lane `recv_timeout` (up to the 250 ms idle cap)
//!    wakes it *immediately*; the resulting snapshot does not wait out the cap.
//!
//! 2. **Single-drain routing (#1231 #3)** — the production loop now routes
//!    through `MailScheduler::drain_command_lane` (the one, non-duplicated drain
//!    implementation) rather than an inline copy. The command dispatched below
//!    is the *replayed* `first_command` path: the blocking `recv_timeout`
//!    dequeues it on wake, holds it, and the next iteration's drain replays it
//!    first. If that replay regressed (command dropped or deferred), the
//!    snapshot would never arrive — or arrive only after the next idle cycle —
//!    and the tight elapsed bound below would fail.
//!
//! With a relay opened (an unreachable URL, so no frames ever flow) the actor
//! settles into the long 250 ms idle wait; the command we send is the *only*
//! thing that can wake it before the cap. A regression to either property
//! shows up as elapsed ≥ ~250 ms.

use crate::actor::{run_actor, ActorCommand, ActorMail, CommandSender};
use crate::transport::wire as fb;
use crate::update_envelope::UpdateFrameBytes;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn is_snapshot(frame: &[u8]) -> bool {
    if !fb::update_frame_buffer_has_identifier(frame) {
        return false;
    }
    match fb::root_as_update_frame(frame) {
        Ok(root) => root.kind() == fb::FrameKind::Snapshot,
        Err(_) => false,
    }
}

/// ADR-0050 §D3a / issue #1231: a command sent to a relay-blocked, idle actor
/// wakes it well under the 250 ms idle cap, and the resulting snapshot is
/// delivered promptly through the single `drain_command_lane` path.
#[test]
fn command_wakes_a_relay_blocked_actor_under_the_idle_cap() {
    let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
    let cmd_tx = CommandSender::new(inbox_tx);
    let (upd_tx, upd_rx) = mpsc::channel::<UpdateFrameBytes>();
    let actor_self_tx = cmd_tx.clone();
    thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));

    // Start the actor with a relay open. The URL is unroutable (TEST-NET-1,
    // RFC 5737) so the worker never connects and no relay frames ever flow —
    // the actor therefore parks in the 250 ms idle `recv_timeout` once its
    // startup state stops changing. `running=true` is required so that a
    // subsequent view-command dispatch emits a snapshot.
    cmd_tx
        .send(ActorCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
            initial_relays: vec![("ws://192.0.2.1:9".to_string(), "read".to_string())],
        })
        .expect("inbox open");

    // Drain the startup snapshots (pre-flight + Start + any settling frames)
    // until the channel goes quiet, i.e. the actor has reached its idle wait.
    // We wait past one full idle cycle so the actor is deep inside the blocking
    // `recv_timeout` when we send the wake command.
    loop {
        match upd_rx.recv_timeout(Duration::from_millis(400)) {
            Ok(_) => continue, // still flushing startup frames
            Err(mpsc::RecvTimeoutError::Timeout) => break, // quiet → actor idle
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("actor exited before reaching idle wait")
            }
        }
    }

    // The actor is now blocked in the 250 ms idle `recv_timeout`. Send a
    // state-mutating command and time how long until its snapshot lands. With
    // the §D3a single waking inbox the command wakes the actor immediately; a
    // regression (command not waking the blocked recv, or the single-drain
    // replay dropping/deferring it) would stall up to the full idle cap.
    let pk = "0".repeat(64);
    let start = Instant::now();
    cmd_tx
        .send(ActorCommand::ClaimProfile {
            pubkey: pk.clone(),
            consumer_id: "wake-test".to_string(),
            force: false,
        })
        .expect("inbox open");

    let frame = upd_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("a snapshot must follow the wake command");
    let elapsed = start.elapsed();

    assert!(
        is_snapshot(&frame),
        "wake command must produce a snapshot frame, not a panic/other frame"
    );
    assert!(
        elapsed < Duration::from_millis(150),
        "command must wake the relay-blocked actor well under the 250 ms idle \
         cap (elapsed: {elapsed:?}); a slower wake means the §D3a single-inbox \
         wake or the #1231 single-drain replay has regressed"
    );

    let _ = cmd_tx.send(ActorCommand::Shutdown);
}
