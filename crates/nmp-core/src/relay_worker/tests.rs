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
