//! Unit test: reconnect preamble — structural REQ-before-EVENT ordering.
//!
//! Extracted from `tests.rs` (file-size split). Pins the guarantee that a
//! preamble registered via `SetReconnectPreamble` is written to the wire
//! BEFORE any `Send` frame queued during the reconnect-wait window.

use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tungstenite::{accept, Message};

use super::tests::drain_until;
use super::{spawn_relay_worker_with_keepalive, RelayCommand, RelayEvent};
use crate::role::RelayRole;

/// Pins the structural guarantee: a preamble registered via
/// `SetReconnectPreamble` is written to the wire BEFORE any `Send` frame
/// that the caller queues during the reconnect-wait window.
///
/// Scenario:
/// 1. Start the worker against a server that accepts ONE connection.
/// 2. Wait for `Connected`, then drop the first connection (forces a reconnect).
/// 3. While the worker is in `wait_before_reconnect`:
///    a. Send `SetReconnectPreamble` with a REQ sentinel.
///    b. Send a `Send` with an EVENT sentinel.
/// 4. Server accepts the second connection and collects the first two frames.
/// 5. Assert the server sees REQ before EVENT.
#[test]
fn reconnect_preamble_arrives_before_queued_send() {
    let (port_tx, port_rx) = mpsc::channel::<u16>();
    let (first_done_tx, first_done_rx) = mpsc::channel::<()>();
    let (frames_tx, frames_rx) = mpsc::channel::<Vec<String>>();

    // ── Server thread ──────────────────────────────────────────────────────
    let _server_thread = thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        port_tx.send(port).ok();

        // ── First connection: handshake then immediately close ─────────────
        let (stream1, _) = listener.accept().expect("accept1");
        stream1
            .set_read_timeout(Some(Duration::from_millis(200)))
            .ok();
        // Completing the upgrade and dropping closes the WS connection.
        drop(accept(stream1));
        first_done_tx.send(()).ok();

        // ── Second connection: collect first 2 text frames ─────────────────
        let (stream2, _) = listener.accept().expect("accept2");
        stream2
            .set_read_timeout(Some(Duration::from_millis(500)))
            .ok();
        let mut ws2 = match accept(stream2) {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut observed: Vec<String> = Vec::new();
        // Max reconnect delay: RELAY_RECONNECT_DELAY_INITIAL (3s) × 2 = 6s
        // + jitter up to 5s = 11s.  Give 15s to absorb load.
        let deadline = Instant::now() + Duration::from_secs(15);
        while observed.len() < 2 && Instant::now() < deadline {
            match ws2.read() {
                Ok(Message::Text(t)) => observed.push(t),
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => break,
            }
        }
        frames_tx.send(observed).ok();
    });

    // ── Worker ─────────────────────────────────────────────────────────────
    let port = port_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("server port");
    let url = format!("ws://127.0.0.1:{port}");

    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
    let control_tx = spawn_relay_worker_with_keepalive(
        RelayRole::Content,
        url,
        1,
        relay_tx,
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    );

    // Wait for first Connected.
    let connected = drain_until(
        &relay_rx,
        |ev| matches!(ev, RelayEvent::Connected { .. }),
        Duration::from_secs(5),
    );
    assert!(connected.is_some(), "worker must connect to first server");

    // Wait for server to confirm it closed the first connection.
    first_done_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("first server close signal");

    // Wait for Failed so the worker is in wait_before_reconnect.
    let failed = drain_until(
        &relay_rx,
        |ev| matches!(ev, RelayEvent::Failed { .. }),
        Duration::from_secs(5),
    );
    assert!(
        failed.is_some(),
        "worker must emit Failed after first server closes"
    );

    // ── Queue preamble + Send while in wait_before_reconnect ──────────────
    // The reconnect backoff is at least ~1 s (RELAY_RECONNECT_DELAY_INITIAL
    // doubled on a fast disconnect), giving plenty of time to queue both
    // commands before the worker re-dials.
    let preamble_frame = r#"["REQ","preamble-sentinel",{"kinds":[24133]}]"#.to_string();
    let send_frame = r#"["EVENT",{"kind":24133,"content":"test"}]"#.to_string();

    control_tx
        .send(RelayCommand::SetReconnectPreamble(vec![
            preamble_frame.clone()
        ]))
        .expect("SetReconnectPreamble must be accepted in wait_before_reconnect");
    control_tx
        .send(RelayCommand::Send(send_frame.clone()))
        .expect("Send must be accepted");

    // ── Assert ordering ────────────────────────────────────────────────────
    // Reconnect takes up to ~11 s (6 s base backoff + 5 s max jitter).
    // Allow 20 s to absorb heavy parallel-test load on CI.
    let observed = frames_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("server did not receive 2 frames on second connection");

    assert!(
        observed.len() >= 2,
        "expected at least 2 frames on second connection; got: {:?}",
        observed
    );
    assert_eq!(
        observed[0], preamble_frame,
        "preamble REQ must arrive BEFORE the queued EVENT — ordering failed; frames={:?}",
        observed
    );
    assert_eq!(
        observed[1], send_frame,
        "queued Send must follow preamble REQ; frames={:?}",
        observed
    );

    let _ = control_tx.send(RelayCommand::Shutdown);
}
