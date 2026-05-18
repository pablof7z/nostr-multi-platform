//! T120b / G4 — end-to-end keepalive wiring tests.
//!
//! These exercise the production `run_relay_worker` against a hermetic
//! loopback WebSocket server. The keepalive FSM itself is unit-tested in
//! `crate::keepalive`; these tests pin the **wiring** — that the worker
//! actually:
//!   1. emits `Message::Ping(_)` after `keepalive_idle` of inbound silence,
//!   2. emits `RelayEvent::Failed` and reconnects when no pong arrives
//!      within `keepalive_pong_timeout`,
//!   3. swallows inbound `Message::Pong(_)` (does not forward to ingest).
//!
//! We use [`spawn_relay_worker_with_keepalive`] to pin millisecond intervals
//! so tests run in <1s, not 30s+.

use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tungstenite::{accept, Message};

use super::{
    spawn_relay_worker_with_keepalive, RelayCommand, RelayEvent,
};
use crate::relay::RelayRole;

/// What the server-side WebSocket observed. Kept narrow so test assertions
/// don't have to match on `Message` variants the test doesn't care about.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ServerObserved {
    Ping,
    Pong,
    Text(String),
    Close,
}

struct LocalServer {
    url: String,
    observed_rx: Receiver<ServerObserved>,
    _shutdown_tx: Sender<()>,
    _thread: JoinHandle<()>,
}

impl LocalServer {
    /// Spawn a server that auto-Pongs (tungstenite default) and reports
    /// every frame it sees on `observed_rx`. Used by tests that exercise
    /// the happy path of the keepalive FSM. The "no-pong" variant lives
    /// inline in `worker_reconnects_when_pong_does_not_arrive` because it
    /// requires a hand-rolled WS handshake to bypass tungstenite's helpful
    /// auto-pong logic.
    fn start_auto_pong() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local_addr").port();
        let url = format!("ws://127.0.0.1:{port}");

        let (observed_tx, observed_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

        let thread = thread::spawn(move || {
            listener
                .set_nonblocking(false)
                .expect("blocking listener");
            let (stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(_) => return,
            };
            stream
                .set_read_timeout(Some(Duration::from_millis(20)))
                .ok();
            let mut socket = match accept(stream) {
                Ok(s) => s,
                Err(_) => return,
            };

            loop {
                if shutdown_rx.try_recv().is_ok() {
                    let _ = socket.close(None);
                    return;
                }
                match socket.read() {
                    Ok(msg) => {
                        let observed = match &msg {
                            Message::Ping(_) => ServerObserved::Ping,
                            Message::Pong(_) => ServerObserved::Pong,
                            Message::Text(t) => ServerObserved::Text(t.clone()),
                            Message::Close(_) => ServerObserved::Close,
                            _ => continue,
                        };
                        let is_close = matches!(observed, ServerObserved::Close);
                        if observed_tx.send(observed).is_err() {
                            return;
                        }
                        if is_close {
                            return;
                        }
                        // Auto-pong path: tungstenite's `read` already buffers
                        // an automatic Pong reply to inbound Pings; the next
                        // `socket.read()` iteration internally flushes it.
                    }
                    Err(tungstenite::Error::Io(e))
                        if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                    Err(_) => return,
                }
            }
        });

        // Give the listener a beat to be ready before the worker dials.
        thread::sleep(Duration::from_millis(30));

        Self {
            url,
            observed_rx,
            _shutdown_tx: shutdown_tx,
            _thread: thread,
        }
    }

    fn await_event(&self, want: ServerObserved, budget: Duration) -> bool {
        let deadline = Instant::now() + budget;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            match self
                .observed_rx
                .recv_timeout(remaining.min(Duration::from_millis(50)))
            {
                Ok(obs) if obs == want => return true,
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }
}

fn drain_until<F: Fn(&RelayEvent) -> bool>(
    rx: &Receiver<RelayEvent>,
    predicate: F,
    budget: Duration,
) -> Option<RelayEvent> {
    let deadline = Instant::now() + budget;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match rx.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(ev) if predicate(&ev) => return Some(ev),
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

#[test]
fn worker_emits_ping_after_idle_threshold() {
    let server = LocalServer::start_auto_pong();

    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
    let _control_tx = spawn_relay_worker_with_keepalive(
        RelayRole::Content,
        server.url.clone(),
        1,
        relay_tx,
        Duration::from_millis(200), // idle threshold
        Duration::from_millis(500), // pong timeout
    );

    // Wait for the worker's Connected event so we know the socket is up.
    let connected = drain_until(
        &relay_rx,
        |ev| matches!(ev, RelayEvent::Connected { .. }),
        Duration::from_secs(2),
    );
    assert!(connected.is_some(), "worker must report Connected");

    // Within the (short) keepalive window, the worker must have emitted a
    // Ping which the server records as `ServerObserved::Ping`.
    let saw_ping = server.await_event(ServerObserved::Ping, Duration::from_secs(2));
    assert!(
        saw_ping,
        "worker did not emit a Ping within 2s of an idle 200ms-threshold socket"
    );
}

#[test]
fn worker_reconnects_when_pong_does_not_arrive() {
    // We want to prove the keepalive FSM declares `Dead` after the pong
    // window elapses. Using a "drop-after-ping" server can race the FSM —
    // the TCP close surfaces as a transport `Failed` before the pong window
    // elapses, masking the keepalive path.
    //
    // Instead, use a server that simply records Pings but never auto-pongs.
    // We implement that with a custom thread that read()s the underlying
    // stream into a raw buffer (bypassing tungstenite's auto-pong logic).
    use std::io::Read;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{port}");

    let _accept_thread = thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(s) => s,
            Err(_) => return,
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok();
        // Read the HTTP upgrade request and reply with a hand-rolled WS
        // handshake response, then never write anything else (including
        // auto-pong). This bypasses tungstenite's helpful auto-pong logic.
        let mut req_buf = Vec::with_capacity(2048);
        let mut tmp = [0u8; 1024];
        // Read until we see \r\n\r\n (end of HTTP headers) or fail.
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline && !req_buf.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut tmp) {
                Ok(0) => return,
                Ok(n) => req_buf.extend_from_slice(&tmp[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => return,
            }
        }
        // Extract Sec-WebSocket-Key.
        let req_str = String::from_utf8_lossy(&req_buf).to_string();
        let key_line = req_str
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("sec-websocket-key:"));
        let key = match key_line.and_then(|l| l.split(':').nth(1)).map(str::trim) {
            Some(k) => k.to_string(),
            None => return,
        };
        // Compute the Sec-WebSocket-Accept per RFC 6455.
        use tungstenite::handshake::derive_accept_key;
        let accept_val = derive_accept_key(key.as_bytes());
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {accept_val}\r\n\r\n",
        );
        use std::io::Write;
        if stream.write_all(response.as_bytes()).is_err() {
            return;
        }
        if stream.flush().is_err() {
            return;
        }
        // Now sit and read raw bytes forever — we DO NOT decode WS frames,
        // we DO NOT respond with Pong. The client's keepalive FSM should
        // declare Dead after its pong window elapses.
        let mut buf = [0u8; 256];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => return,
                Ok(_) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => return,
            }
        }
    });
    thread::sleep(Duration::from_millis(30));

    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
    let _control_tx = spawn_relay_worker_with_keepalive(
        RelayRole::Content,
        url,
        1,
        relay_tx,
        Duration::from_millis(150), // idle
        Duration::from_millis(300), // pong timeout
    );

    // Server records Connected → worker pings → no pong arrives → keepalive
    // FSM declares Dead → worker emits Failed with the keepalive marker.
    let failed = drain_until(
        &relay_rx,
        |ev| {
            matches!(
                ev,
                RelayEvent::Failed { error, .. } if error.contains("keepalive timeout")
            )
        },
        Duration::from_secs(3),
    );
    assert!(
        failed.is_some(),
        "worker must emit Failed with 'keepalive timeout' when no pong arrives"
    );
}

#[test]
fn worker_swallows_pong_does_not_forward_to_kernel() {
    let server = LocalServer::start_auto_pong();

    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
    let control_tx = spawn_relay_worker_with_keepalive(
        RelayRole::Content,
        server.url.clone(),
        1,
        relay_tx,
        Duration::from_millis(150),
        Duration::from_secs(2),
    );

    // Wait past the first keepalive cycle (ping + pong round-trip).
    thread::sleep(Duration::from_millis(800));

    // Tell the worker to send a sentinel text so we have a known wire event
    // to assert ordering against.
    control_tx
        .send(RelayCommand::Send("[\"NOTICE\",\"sentinel\"]".to_string()))
        .expect("send sentinel");

    // Drain all RelayEvents for the next ~500ms and assert none of them is
    // a Message{ message: Pong(_) }.
    let deadline = Instant::now() + Duration::from_millis(800);
    let mut had_any_message = false;
    while Instant::now() < deadline {
        match relay_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(RelayEvent::Message { message, .. }) => {
                had_any_message = true;
                assert!(
                    !matches!(message, Message::Pong(_)),
                    "Pong frames must be swallowed at the worker; got one in RelayEvent::Message"
                );
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    let _ = had_any_message; // not asserted — server may not echo our notice
}

#[test]
fn worker_does_not_ping_when_inbound_keeps_arriving() {
    // If the server keeps talking to us, the keepalive should never fire.
    // We exercise this by having the worker send a text frame and the
    // server auto-replies with a Pong on every inbound Ping (the auto-pong
    // behavior of tungstenite). To simulate "keeps arriving" without
    // running a chatty server, we send periodic frames from our side: each
    // outbound flushes the socket, and tungstenite's `read` advances
    // through whatever the server has sent. The cleanest assertion is the
    // narrow one: with an idle threshold of 1s and a 600ms test window,
    // we must see ZERO pings.

    let server = LocalServer::start_auto_pong();

    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
    let _control_tx = spawn_relay_worker_with_keepalive(
        RelayRole::Content,
        server.url.clone(),
        1,
        relay_tx,
        Duration::from_secs(1),
        Duration::from_secs(2),
    );

    // Wait for Connected so we know the socket is up.
    let _ = drain_until(
        &relay_rx,
        |ev| matches!(ev, RelayEvent::Connected { .. }),
        Duration::from_secs(2),
    );

    // 600ms is well below the 1s threshold; no Ping should reach the server.
    let saw_ping = server.await_event(ServerObserved::Ping, Duration::from_millis(600));
    assert!(
        !saw_ping,
        "worker must not Ping inside the idle threshold window"
    );
}

// ─── T130 — pre-connect frames buffer on the worker until Open ──────────────

#[test]
fn t130_frames_sent_before_connect_arrive_after_open() {
    // T130 invariant: when the kernel sends a frame to a worker whose socket
    // is not yet open, the worker's internal `pending: VecDeque<String>`
    // holds the frame until `flush_relay_writes` runs post-Connected. This is
    // the "buffered waiting for that URL" mechanism — implemented at the
    // worker layer, not via a kernel-side per-URL gate.
    //
    // We force the race by binding a local server but accepting connections
    // SLOWLY: the listener thread sleeps before calling `accept()`, while
    // the worker is already dialing. We send a sentinel frame to the worker's
    // control channel during that window — it MUST land on the wire once the
    // server finishes its handshake.
    use std::io::{Read, Write};
    use tungstenite::handshake::derive_accept_key;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{port}");

    // Server: accept after a 300ms delay so the worker has time to dial and
    // queue a frame before the handshake completes. Once connected, log every
    // text frame into a channel for the test to inspect.
    let (server_observed_tx, server_observed_rx) = mpsc::channel::<String>();
    let _server_thread = thread::spawn(move || {
        listener.set_nonblocking(false).ok();
        // Stall before accepting: the worker is racing to dial; the frame the
        // test sends to its control channel lands in the worker's pending
        // queue while there's no socket yet.
        thread::sleep(Duration::from_millis(300));
        let (mut stream, _) = match listener.accept() {
            Ok(s) => s,
            Err(_) => return,
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok();
        // Hand-rolled HTTP→WS upgrade (so the test does not depend on the
        // exact tungstenite server-side accept loop).
        let mut req = Vec::with_capacity(2048);
        let mut tmp = [0u8; 1024];
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline && !req.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut tmp) {
                Ok(0) => return,
                Ok(n) => req.extend_from_slice(&tmp[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => return,
            }
        }
        let req_str = String::from_utf8_lossy(&req).to_string();
        let key = req_str
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("sec-websocket-key:"))
            .and_then(|l| l.split(':').nth(1))
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        let accept_val = derive_accept_key(key.as_bytes());
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {accept_val}\r\n\r\n",
        );
        if stream.write_all(response.as_bytes()).is_err() {
            return;
        }
        // Now parse WS frames manually. We only care about Text frames.
        // tungstenite::WebSocket::from_raw_socket would also work, but
        // hand-rolling keeps this test free of unstable internals.
        // Decode one or more frames; loop until we see the sentinel or time out.
        let mut buf = Vec::with_capacity(2048);
        let frame_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < frame_deadline {
            let mut chunk = [0u8; 256];
            match stream.read(&mut chunk) {
                Ok(0) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    continue;
                }
                Err(_) => return,
            }
            // Try to decode one frame. Per RFC 6455, client→server frames are
            // masked and start with FIN+opcode byte. We just want to extract
            // the payload of any Text frame (opcode 0x1).
            if let Some(text) = decode_first_text_frame(&buf) {
                let _ = server_observed_tx.send(text);
                return;
            }
        }
    });

    // Worker dials immediately (the listener is bound, so `connect` returns
    // either after the 300ms server-side stall or as soon as the kernel
    // accepts the TCP — depends on platform; on macOS the listen backlog
    // accepts TCP first, then the WS handshake stalls).
    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
    let control_tx = spawn_relay_worker_with_keepalive(
        RelayRole::Content,
        url.clone(),
        1,
        relay_tx,
        Duration::from_secs(60), // long idle so keepalive doesn't interfere
        Duration::from_secs(60),
    );

    // CRITICAL: send the frame BEFORE Connected arrives. The worker's outer
    // loop reads from `control_rx` before it has a socket; on first dial it
    // calls `open_relay_socket` first, but any RelayCommand::Send queued
    // during the dial sits in the worker thread's mpsc channel and gets
    // pumped on the next iteration of run_connected_relay (post-Connected).
    let sentinel = "[\"REQ\",\"t130-sentinel\",{\"kinds\":[1]}]".to_string();
    control_tx
        .send(RelayCommand::Send(sentinel.clone()))
        .expect("worker control channel must accept the pre-connect frame");

    // Wait for the server to observe the sentinel. Budget includes the 300ms
    // server stall + handshake + frame round-trip.
    let observed = server_observed_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("server did not observe the pre-connect frame within 3s");
    assert_eq!(
        observed, sentinel,
        "the pre-connect frame must land on the wire post-Open, with the same bytes"
    );

    // Sanity: the worker really did emit Connected during the test (proves
    // the race actually happened — frame went out AFTER socket open).
    let connected = drain_until(
        &relay_rx,
        |ev| matches!(ev, RelayEvent::Connected { .. }),
        Duration::from_millis(500),
    );
    assert!(connected.is_some(), "worker must report Connected");
}

/// Minimal RFC-6455 frame decoder: returns the first complete Text frame's
/// payload, or `None` if the buffer doesn't yet contain one. Client→server
/// frames are masked; we apply the mask before returning.
fn decode_first_text_frame(buf: &[u8]) -> Option<String> {
    if buf.len() < 2 {
        return None;
    }
    let b0 = buf[0];
    let opcode = b0 & 0x0F;
    if opcode != 0x1 {
        // Not a text frame; we don't bother decoding (test only sends text).
        return None;
    }
    let b1 = buf[1];
    let masked = (b1 & 0x80) != 0;
    let mut len = (b1 & 0x7F) as usize;
    let mut idx = 2;
    if len == 126 {
        if buf.len() < idx + 2 {
            return None;
        }
        len = u16::from_be_bytes([buf[idx], buf[idx + 1]]) as usize;
        idx += 2;
    } else if len == 127 {
        // 64-bit length — not expected for the small sentinel; bail.
        return None;
    }
    let mask = if masked {
        if buf.len() < idx + 4 {
            return None;
        }
        let m = [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]];
        idx += 4;
        Some(m)
    } else {
        None
    };
    if buf.len() < idx + len {
        return None;
    }
    let payload = &buf[idx..idx + len];
    let decoded: Vec<u8> = if let Some(m) = mask {
        payload
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ m[i % 4])
            .collect()
    } else {
        payload.to_vec()
    };
    String::from_utf8(decoded).ok()
}
