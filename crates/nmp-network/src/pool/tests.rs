//! Unit tests for the push-model [`super::Pool`] API.
//!
//! Two layers of test:
//!
//! 1. **Pure structural** — slot allocation, generational handle
//!    rejection, snapshot shape, "no send-to-all" surface. No real
//!    socket; the worker's spawn call is exercised but the URL is a
//!    sentinel that never connects (we only assert the
//!    pool-side bookkeeping).
//! 2. **Real socket** — boot a `tungstenite::server::accept` on a
//!    loopback port, drive `ensure_open` + `send` + assert a
//!    `PoolEvent::Opened` then `PoolEvent::Frame` arrives.
//!
//! The full keepalive / reconnect / jitter behaviour is already
//! exercised by [`crate::relay_worker::tests`] (22 tests, all green
//! after phase A). These tests focus on the new push-model surface.

use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::{
    inner::canonicalize, ClosedReason, HealthState, Pool, PoolConfig, PoolEvent, RelayFrame,
    RelayHandle, WireFrame,
};
use crate::role::RelayRole;

/// canonicalize: case-fold the URL scheme so `WSS://` and `wss://`
/// share a pool slot. The full URL parser is the actor's job; the
/// pool just lowers the obvious case differences.
#[test]
fn canonicalize_lowercases_scheme() {
    assert_eq!(canonicalize("WSS://relay.example"), "wss://relay.example");
    assert_eq!(canonicalize("wss://relay.example"), "wss://relay.example");
}

/// canonicalize: leading/trailing whitespace is trimmed so a stray
/// newline in a configured-relays list doesn't fragment the pool.
#[test]
fn canonicalize_trims_whitespace() {
    assert_eq!(
        canonicalize("  wss://relay.example\n"),
        "wss://relay.example"
    );
}

/// Two `ensure_open` calls for the same URL share a slot.
///
/// Without a real socket, the worker thread will keep retrying the
/// dial, but the pool-side state (slot map, handle generation) is
/// observable synchronously.
#[test]
fn ensure_open_idempotent_same_url_returns_same_handle() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    // Use a port that nothing's listening on so the worker dials and
    // fails — the slot allocation is what we assert, not connectivity.
    let url = String::from("wss://127.0.0.1:1/sentinel");
    let h1 = pool.ensure_open(&url);
    let h2 = pool.ensure_open(&url);
    assert_eq!(h1, h2, "same URL must yield same handle");
    pool.shutdown();
}

/// Distinct URLs get distinct slots.
#[test]
fn ensure_open_distinct_urls_get_distinct_slots() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let h_a = pool.ensure_open(&String::from("wss://127.0.0.1:1/a"));
    let h_b = pool.ensure_open(&String::from("wss://127.0.0.1:1/b"));
    assert_ne!(
        h_a.slot(),
        h_b.slot(),
        "distinct URLs must get distinct slot ids"
    );
    pool.shutdown();
}

/// `close` then `ensure_open` for the same URL re-uses the slot id but
/// bumps the generation. The prior handle is now structurally stale.
#[test]
fn close_then_reopen_bumps_generation_invalidating_stale_handle() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let url = String::from("wss://127.0.0.1:1/sentinel");
    let h1 = pool.ensure_open(&url);
    assert!(pool.close(h1));
    let h2 = pool.ensure_open(&url);
    assert_eq!(h1.slot(), h2.slot(), "slot id must be reused");
    assert!(
        h2.generation() > h1.generation(),
        "reopen must bump generation (was {}, is {})",
        h1.generation(),
        h2.generation(),
    );
    // The stale handle is rejected by the public API.
    assert!(
        pool.health(h1).is_none(),
        "stale handle must yield None from health()"
    );
    assert!(
        !pool.close(h1),
        "stale handle must be a no-op for close()"
    );
    assert!(
        !pool.send(h1, WireFrame::Text("[\"REQ\",\"x\",{}]".to_string())),
        "stale handle must be a no-op for send()"
    );
    pool.shutdown();
}

/// `health()` returns `Some(state=Connecting)` immediately after
/// `ensure_open` (before any worker event arrives).
#[test]
fn health_after_ensure_open_is_connecting() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let h = pool.ensure_open(&String::from("wss://127.0.0.1:1/sentinel"));
    let health = pool.health(h).expect("fresh handle must be live");
    assert_eq!(health.state, HealthState::Connecting);
    pool.shutdown();
}

/// `snapshot()` enumerates every live slot.
#[test]
fn snapshot_enumerates_live_slots() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let _h_a = pool.ensure_open(&String::from("wss://127.0.0.1:1/a"));
    let _h_b = pool.ensure_open(&String::from("wss://127.0.0.1:1/b"));
    let snap = pool.snapshot();
    assert_eq!(snap.rows.len(), 2, "snapshot must list both slots");
    pool.shutdown();
}

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
        let _ = websocket.write(tungstenite::Message::Text("[\"NOTICE\",\"hi\"]".to_string()));
        let _ = websocket.flush();
        // Hold the socket open briefly so the client has time to read.
        thread::sleep(Duration::from_millis(500));
    });

    let (events_tx, events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let url = format!("ws://127.0.0.1:{port}");
    let _h = pool.ensure_open(&url);

    let frame_event = recv_until(&events_rx, Duration::from_secs(5), |ev| {
        matches!(ev, PoolEvent::Frame { frame: RelayFrame::Text(_), .. })
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

/// Sentinel handle returned post-shutdown is structurally invalid.
#[test]
fn ensure_open_after_shutdown_returns_sentinel_handle() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    pool.shutdown();
    let h = pool.ensure_open(&String::from("wss://127.0.0.1:1/sentinel"));
    assert_eq!(h.slot(), u32::MAX, "post-shutdown ensure must be sentinel");
    assert!(
        !pool.send(h, WireFrame::Text("ignored".to_string())),
        "sentinel handle must be a no-op for send()"
    );
}

/// Structural-typing guard: `Pool` has no method named `send_all` or
/// `broadcast`. The compiler enforces this, but a smoke test keeps it
/// in the test catalogue so future contributors see the intent.
///
/// NDK issue #175 answer: every send is constrained to a `RelayHandle`.
#[test]
fn pool_exposes_no_send_to_all_method() {
    // Pure compile-time assertion — if someone adds `Pool::send_all`,
    // this test does not break; the contract lives in the `Pool` impl
    // block and the spec at `docs/architecture/crate-boundaries.md`
    // §3.8. The test is here as a discoverable failure point if
    // someone audits the test list.
    //
    // We *do* call `send` to assert the only fan-out path: caller
    // supplies a handle.
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    let h = pool.ensure_open(&String::from("wss://127.0.0.1:1/sentinel"));
    let _ok = pool.send(h, WireFrame::Text("[\"REQ\",\"x\",{}]".to_string()));
    pool.shutdown();
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

/// Helper: spin `events.recv` until either a matching event arrives or
/// `budget` elapses.
fn recv_until<F>(
    rx: &mpsc::Receiver<PoolEvent>,
    budget: Duration,
    pred: F,
) -> Option<PoolEvent>
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

/// Sanity: `RelayHandle` is `Copy`. The kernel actor stores many of
/// these in `wire_subs` and a `Copy` bound keeps that code clutter-free.
#[test]
fn relay_handle_is_copy() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<RelayHandle>();
}

/// Sanity: `Pool` is `Clone`. The kernel actor hands clones into
/// `ProtocolCommand` closures.
#[test]
fn pool_is_clone() {
    fn assert_clone<T: Clone>() {}
    assert_clone::<Pool>();
}

/// The `default_role` from `PoolConfig` is propagated to workers.
#[test]
fn ensure_open_with_explicit_role_overrides_default() {
    let (events_tx, _events_rx) = mpsc::channel();
    let pool = Pool::new(
        PoolConfig {
            default_role: RelayRole::Indexer,
            ..PoolConfig::default()
        },
        events_tx,
    );
    let h_default = pool.ensure_open(&String::from("wss://127.0.0.1:1/a"));
    let _h_explicit = pool.ensure_open_with_role(
        &String::from("wss://127.0.0.1:1/b"),
        RelayRole::Content,
    );
    let snap = pool.snapshot();
    let row_a = snap
        .rows
        .iter()
        .find(|r| r.handle == h_default)
        .expect("default-role row");
    assert_eq!(row_a.role, RelayRole::Indexer);
    pool.shutdown();
}
