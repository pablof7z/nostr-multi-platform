//! Shared test fixtures for the `relay_worker` test suite: a hermetic
//! loopback WebSocket server (`LocalServer`) that records the frames it
//! observes (`ServerObserved`), and `drain_until`, a poll-until-predicate
//! helper for the worker's `RelayEvent` channel. Used by this crate's
//! `tests` submodules and by the sibling `control_disconnect_tests` /
//! `preamble_tests` suites in `relay_worker`.
use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tungstenite::{accept, Message};

use crate::relay_worker::RelayEvent;

/// What the server-side WebSocket observed. Kept narrow so test assertions
/// don't have to match on `Message` variants the test doesn't care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::relay_worker) enum ServerObserved {
    Ping,
    Pong,
    Text(String),
    Close,
}

pub(in crate::relay_worker) struct LocalServer {
    pub(in crate::relay_worker) url: String,
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
    pub(in crate::relay_worker) fn start_auto_pong() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local_addr").port();
        let url = format!("ws://127.0.0.1:{port}");

        let (observed_tx, observed_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let thread = thread::spawn(move || {
            listener.set_nonblocking(false).expect("blocking listener");
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
                        // tungstenite buffers auto-Pong on read; the next read
                        // iteration internally flushes it.
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

    pub(in crate::relay_worker) fn await_event(&self, want: ServerObserved, budget: Duration) -> bool {
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

pub(in crate::relay_worker) fn drain_until<F: Fn(&RelayEvent) -> bool>(
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
