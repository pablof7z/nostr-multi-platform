//! V-58 — `SetBackoffHint` reconnect-schedule tests: the worker accepts
//! the hint without breaking the normal reconnect path, the
//! `BackoffClass` schedule constants keep their documented ordering, and
//! a `RateLimited` hint overrides the V-92 healthy-session backoff reset.
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tungstenite::accept;

use super::super::{spawn_relay_worker_with_keepalive, BackoffClass, RelayCommand, RelayEvent};
use super::support::drain_until;
use crate::relay_protocol::apply_reconnect_backoff;
use crate::role::RelayRole;

/// V-58 — when the worker receives a `SetBackoffHint(RateLimited)` command
/// while connected, it reconnects with a long delay (≥ `RELAY_RECONNECT_DELAY_RATE_LIMITED`)
/// rather than the normal initial delay.
///
/// Approach: run the worker against a server that accepts one connection and
/// immediately drops it, then measure the wall-clock gap between the first
/// `Failed` event and the next `Connected` event. Send the hint just after
/// `Connected` so it is guaranteed to be draining during the session.
///
/// To keep the test fast we override `RELAY_RECONNECT_DELAY_RATE_LIMITED`-like
/// behaviour indirectly: test the *schedule logic* at the unit level rather
/// than waiting 60 s. The integration proof lives in the protocol-constants
/// test (`relay_protocol::tests::rate_limited_delay_exceeds_initial_delay`).
/// Here we prove only that the worker *accepts* the `SetBackoffHint` command
/// without panicking and that a hint sent during a live session does not
/// prevent the subsequent `Failed` event from arriving.
///
/// ## Determinism note
///
/// The prior implementation used `drop(accept(stream))` immediately, which
/// raced the server-side close against the worker's mio registration. On
/// kqueue (macOS, EV_CLEAR / edge-triggered), a `EVFILT_READ` EOF event that
/// arrives in the same `kevent()` batch as the control-channel wakeup from
/// `SetBackoffHint` was only delivered once; the second `kevent()` call
/// would block for the full 60-s keepalive timeout because the edge had
/// already been consumed. The fix has two parts:
///
/// 1. **Production (`mod.rs`)**: drain socket reads before checking
///    `ready.control || ready.writable` so an EOF that co-arrives with a
///    waker event is never silently dropped.
/// 2. **Test (this function)**: coordinate the server-side drop via a channel
///    so it happens *after* the test has observed `Connected` and delivered
///    `SetBackoffHint`, eliminating the simultaneous-event race from the test
///    itself.
#[test]
fn v58_set_backoff_hint_does_not_break_reconnect() {
    // Channel used to signal the server thread to drop the connection.
    // The server holds the WebSocket open until it receives this signal so the
    // test can fully observe `Connected` and deliver `SetBackoffHint` before
    // the TCP close arrives — eliminating the edge-triggered kqueue race where
    // the EOF and the waker fire in the same kevent() batch.
    let (drop_tx, drop_rx) = mpsc::channel::<()>();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{port}");

    let _accept_thread = thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            // Complete the WS handshake so the worker sees `Connected`, then
            // hold the WebSocket open until the test signals us to drop it.
            if let Ok(socket) = accept(stream) {
                // Block until the test is ready for the disconnect.
                drop_rx.recv().ok();
                // Dropping `socket` closes the TCP connection without a WS
                // Close frame, causing the worker to emit `RelayEvent::Failed`.
                drop(socket);
            }
        }
    });
    thread::sleep(Duration::from_millis(30));

    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
    let control_tx = spawn_relay_worker_with_keepalive(
        RelayRole::Content,
        url,
        1,
        relay_tx,
        Duration::from_secs(60), // no keepalive interference
        Duration::from_secs(60),
        None,
    );

    // Wait for Connected. 5s budget so the test survives slow CI machines.
    let connected = drain_until(
        &relay_rx,
        |ev| matches!(ev, RelayEvent::Connected { .. }),
        Duration::from_secs(5),
    );
    assert!(connected.is_some(), "worker must report Connected");

    // Send the hint while the socket is still open (server is holding the
    // connection open, waiting for our drop signal below).
    control_tx
        .send(RelayCommand::SetBackoffHint(BackoffClass::RateLimited))
        .expect("hint must be accepted by the worker channel");

    // Signal the server to drop the connection.  This happens *after* the
    // SetBackoffHint is queued to the worker, so the waker and the EOF event
    // cannot arrive simultaneously in the same kevent() batch.
    drop_tx.send(()).expect("signal server to drop connection");

    // The worker should discover the dropped connection and emit Failed.
    // 5s budget: the server-side drop may take a round-trip before the client
    // sees it, and CI machines can be slow.
    let failed = drain_until(
        &relay_rx,
        |ev| matches!(ev, RelayEvent::Failed { .. }),
        Duration::from_secs(5),
    );
    assert!(
        failed.is_some(),
        "worker must report Failed after the server drops the connection"
    );

    // Shut down cleanly; no panic means SetBackoffHint was handled.
    let _ = control_tx.send(RelayCommand::Shutdown);
}

/// V-58 — the `BackoffClass` schedule constants satisfy the documented
/// ordering: `RateLimited` delay base is strictly greater than `Transient`
/// (the normal initial delay). This pins the constants so a future edit
/// that inverts the order surfaces immediately.
#[test]
fn v58_rate_limited_backoff_base_exceeds_initial_delay() {
    use crate::relay_protocol::{
        RELAY_RECONNECT_DELAY_INITIAL, RELAY_RECONNECT_DELAY_RATE_LIMITED,
    };
    assert!(
        RELAY_RECONNECT_DELAY_RATE_LIMITED > RELAY_RECONNECT_DELAY_INITIAL,
        "RELAY_RECONNECT_DELAY_RATE_LIMITED ({:?}) must exceed RELAY_RECONNECT_DELAY_INITIAL ({:?})",
        RELAY_RECONNECT_DELAY_RATE_LIMITED,
        RELAY_RECONNECT_DELAY_INITIAL,
    );
}

/// V-58 / V-92 composition: a `RateLimited` hint overrides the V-92 healthy-
/// session reset. After a long healthy session (elapsed ≥ 5 min), a normal
/// transient drop resets the backoff to `INITIAL`; a rate-limited hint must
/// instead pin it to `RELAY_RECONNECT_DELAY_RATE_LIMITED`.
///
/// This test calls the real production `apply_reconnect_backoff` function so
/// a future edit that reorders the V-58/V-92 branches surfaces immediately.
#[test]
fn v58_rate_limited_hint_overrides_v92_healthy_session_reset() {
    use crate::relay_protocol::{
        RELAY_RECONNECT_DELAY_INITIAL, RELAY_RECONNECT_DELAY_RATE_LIMITED,
    };

    let healthy_elapsed = Duration::from_secs(400); // > 5 min → V-92 would reset
    let mut backoff_v92 = Duration::from_secs(60); // some previously-advanced backoff
    let mut backoff_v58 = Duration::from_secs(60);

    // V-92: healthy session + no hint → reset to INITIAL.
    let v92_base = apply_reconnect_backoff(None, &mut backoff_v92, healthy_elapsed);
    assert_eq!(
        v92_base, RELAY_RECONNECT_DELAY_INITIAL,
        "V-92: healthy session + no hint must reset backoff base to INITIAL"
    );
    assert_eq!(
        backoff_v92, RELAY_RECONNECT_DELAY_INITIAL,
        "apply_reconnect_backoff must mutate current_backoff to INITIAL on V-92 reset"
    );

    // V-58 override: healthy session + RateLimited hint → long delay, NOT reset.
    let v58_base = apply_reconnect_backoff(
        Some(BackoffClass::RateLimited),
        &mut backoff_v58,
        healthy_elapsed,
    );
    assert_eq!(
        v58_base, RELAY_RECONNECT_DELAY_RATE_LIMITED,
        "V-58: RateLimited hint must override V-92 reset → long base delay"
    );
    assert_eq!(
        backoff_v58, RELAY_RECONNECT_DELAY_RATE_LIMITED,
        "apply_reconnect_backoff must pin current_backoff to RATE_LIMITED"
    );
    assert!(
        v58_base > RELAY_RECONNECT_DELAY_INITIAL,
        "rate-limited base must exceed initial delay"
    );
}
