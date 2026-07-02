//! Real-socket end-to-end tests: boot a `tungstenite::server::accept` on a
//! loopback port, drive `ensure_open` + `send` + `close`, and assert the
//! `PoolEvent` stream (`Opened`, `Frame`, `Closed`) plus the post-shutdown
//! consumer-channel disconnect guarantee.
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::super::{ClosedReason, Pool, PoolConfig, PoolEvent, RelayFrame, WireFrame};

/// Real-socket end-to-end: the pool dials a loopback relay, emits an
/// `Opened`, the actor `send`s a text frame, and we read it server-side.
#[test]
fn end_to_end_pool_opens_socket_and_delivers_outbound_frame() {
    // Reuse the test scaffolding pattern from
    // `crate::relay_worker::tests`: a single-shot tungstenite
    // server on a loopback port. We accept one socket, read one
    // text frame, and signal success back to the test thread.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    let (server_done_tx, server_done_rx) = mpsc::channel::<String>();
    let server_handle = thread::spawn(move || {
        let (stream, _addr) = listener.accept().expect("accept");
        let mut websocket = tungstenite::accept(stream).expect("ws handshake");
        // Read one text frame from the client, forward it to the test.
        loop {
            match websocket.read() {
                Ok(tungstenite::Message::Text(text)) => {
                    let _ = server_done_tx.send(text);
                    break;
                }
                Ok(tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_)) => continue,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    });

    let (events_tx, events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let url = format!("ws://127.0.0.1:{port}");
    let h = pool.ensure_open(&url);

    // Wait for Opened.
    let opened = recv_until(&events_rx, Duration::from_secs(5), |ev| {
        matches!(ev, PoolEvent::Opened { .. })
    })
    .expect("PoolEvent::Opened within 5s");
    match opened {
        PoolEvent::Opened {
            h: opened_h,
            generation,
            ..
        } => {
            assert_eq!(opened_h, h, "Opened must carry the live handle");
            assert_eq!(generation, h.generation());
        }
        other => panic!("expected Opened, got {other:?}"),
    }

    // Send a text frame and assert the server received it.
    let payload = "[\"REQ\",\"sub1\",{\"limit\":1}]".to_string();
    assert!(pool.send(h, WireFrame::Text(payload.clone())));
    let received = server_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server must receive the text frame within 5s");
    assert_eq!(received, payload);

    pool.shutdown();
    let _ = server_handle.join();
}

/// Real-socket inbound: the pool surfaces a server-emitted text frame
/// as a `PoolEvent::Frame { frame: RelayFrame::Text(...) }`.
#[test]
fn end_to_end_pool_surfaces_inbound_text_as_relay_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    let server_handle = thread::spawn(move || {
        let (stream, _addr) = listener.accept().expect("accept");
        let mut websocket = tungstenite::accept(stream).expect("ws handshake");
        // Push one text frame at the client.
        let _ = websocket.write(tungstenite::Message::Text(
            "[\"NOTICE\",\"hi\"]".to_string(),
        ));
        let _ = websocket.flush();
        // Hold the socket open briefly so the client has time to read.
        thread::sleep(Duration::from_millis(500));
    });

    let (events_tx, events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let url = format!("ws://127.0.0.1:{port}");
    let _h = pool.ensure_open(&url);

    let frame_event = recv_until(&events_rx, Duration::from_secs(5), |ev| {
        matches!(
            ev,
            PoolEvent::Frame {
                frame: RelayFrame::Text(_),
                ..
            }
        )
    })
    .expect("PoolEvent::Frame(Text) within 5s");
    match frame_event {
        PoolEvent::Frame {
            frame: RelayFrame::Text(text),
            ..
        } => {
            assert_eq!(text, "[\"NOTICE\",\"hi\"]");
        }
        other => panic!("expected Frame(Text), got {other:?}"),
    }

    pool.shutdown();
    let _ = server_handle.join();
}

/// After a `close`, the consumer receives a `PoolEvent::Closed` with
/// `reason = Requested`.
#[test]
fn close_emits_closed_event() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    let server_handle = thread::spawn(move || {
        let (stream, _addr) = listener.accept().expect("accept");
        let mut websocket = tungstenite::accept(stream).expect("ws handshake");
        // Keep the socket open until the client closes it.
        while websocket.read().is_ok() {}
    });

    let (events_tx, events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let url = format!("ws://127.0.0.1:{port}");
    let h = pool.ensure_open(&url);
    let _opened = recv_until(&events_rx, Duration::from_secs(5), |ev| {
        matches!(ev, PoolEvent::Opened { .. })
    })
    .expect("Opened within 5s");

    assert!(pool.close(h));
    let closed = recv_until(&events_rx, Duration::from_secs(5), |ev| {
        matches!(ev, PoolEvent::Closed { .. })
    })
    .expect("Closed within 5s");
    match closed {
        PoolEvent::Closed { reason, .. } => assert_eq!(reason, ClosedReason::Requested),
        other => panic!("expected Closed, got {other:?}"),
    }

    pool.shutdown();
    let _ = server_handle.join();
}

/// Regression for a bunker-signing teardown deadlock that shipped briefly in
/// PR #477 (in the since-deleted `nmp-signer-broker`): `Pool::shutdown` MUST
/// drop the public events sender, not just the worker-event sender. A consumer
/// that holds the `Pool` while joining a dispatcher thread blocked on
/// `events_rx.recv()` would hang forever if `shutdown` only dropped the worker
/// channel — the events sender stays alive on `PoolInner.events` (kept by the
/// `Pool`'s inner `Arc<Mutex<_>>`). This invariant still guards every `Pool`
/// consumer, including the `nmp-nip46-runtime` actor-lane transport.
#[test]
fn shutdown_drops_public_events_sender_for_consumer_join() {
    let (events_tx, events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    // Hold the pool alive AFTER shutdown — this mirrors the broker:
    // `PoolRelayClient::shutdown` calls `pool.shutdown()` and then
    // joins its dispatcher while still owning the `Pool`.
    pool.shutdown();
    // The receiver MUST observe disconnection even though `pool` is
    // still in scope. A `recv()` here would block forever without the
    // events-sender swap inside `PoolInner::shutdown`.
    match events_rx.recv() {
        Err(_) => {}
        Ok(ev) => panic!("expected Disconnected after shutdown, got {ev:?}"),
    }
    // Keep `pool` alive until past the assertion so the compiler
    // can't move-drop it early.
    drop(pool);
}

/// Helper: spin `events.recv` until either a matching event arrives or
/// `budget` elapses.
fn recv_until<F>(rx: &mpsc::Receiver<PoolEvent>, budget: Duration, pred: F) -> Option<PoolEvent>
where
    F: Fn(&PoolEvent) -> bool,
{
    let deadline = Instant::now() + budget;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match rx.recv_timeout(remaining) {
            Ok(ev) => {
                if pred(&ev) {
                    return Some(ev);
                }
            }
            Err(_) => return None,
        }
    }
}
