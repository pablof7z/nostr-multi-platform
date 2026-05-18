//! Minimal blocking relay client used by the broker.
//!
//! ## Why a separate client?
//!
//! `nmp-core` has its own production relay-worker pool (`relay_worker.rs`)
//! optimised for the kernel's outbox / inbox topology: per-URL workers, a
//! `RelayCommand` channel, role-keyed routing, etc. Reusing it would require
//! threading `Sender<RelayCommand>` through the broker plus an inbound
//! filter for kind:24133, which is hard to do without leaking NIP-46
//! specifics into the kernel (D0).
//!
//! The broker's needs are simpler: ONE persistent WebSocket per active
//! bunker session, one outbox queue, one inbox subscription, and a callback
//! for inbound kind:24133 frames. A self-contained client lets the broker
//! own the socket end-to-end without modifying `nmp-core`.
//!
//! ## Protocol shape
//!
//! - Subscribe with a NIP-01 `REQ` envelope: `["REQ", <sub_id>, {filter}]`
//!   where `filter = {"kinds":[24133], "#p":[local_pubkey_hex], "since": <epoch>}`.
//! - Publish with `["EVENT", <signed_event_json>]`.
//! - Inbound frames arrive as `["EVENT", <sub_id>, <event_json>]`; the client
//!   parses these and calls the registered `event_callback`. Other frame
//!   types (`EOSE`, `NOTICE`, `OK`, `CLOSED`) are ignored for MVP.
//!
//! ## Trait surface
//!
//! [`RelayClient`] is a trait so unit tests can stub the transport without
//! spinning up a TCP listener. The production impl
//! [`TungsteniteRelayClient`] uses a blocking `tungstenite` socket on its
//! own thread.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Once;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::Value;

/// Signature of the inbound event callback. Receives the raw event JSON
/// `Value` (the third element of `["EVENT", <sub_id>, <event_json>]`).
/// MUST be cheap (called on the relay-client thread); offload work if needed.
pub type EventCallback = Arc<dyn Fn(Value) + Send + Sync>;

/// Errors returned from the relay client. String-typed to keep the surface
/// small; the broker converts these to `BunkerHandshakeProgress` failures
/// via `Display`.
#[derive(Debug)]
pub enum RelayError {
    /// Connection / handshake failed (TLS, TCP, WebSocket upgrade).
    Connect(String),
    /// Socket write failure during a `publish` call.
    Write(String),
    /// Background thread terminated; the client is no longer usable.
    Disconnected,
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelayError::Connect(m) => write!(f, "connect failed: {m}"),
            RelayError::Write(m) => write!(f, "write failed: {m}"),
            RelayError::Disconnected => f.write_str("relay client disconnected"),
        }
    }
}

impl std::error::Error for RelayError {}

/// Trait the broker programs against. Production: [`TungsteniteRelayClient`].
/// Tests: stub with a `Vec`-backed sink.
pub trait RelayClient: Send + Sync {
    /// Send a raw NIP-01 client frame (`["REQ", ...]`, `["EVENT", ...]`,
    /// `["CLOSE", ...]`). The broker constructs the JSON itself.
    fn send(&self, frame: String) -> Result<(), RelayError>;

    /// Cancel the worker, close the socket. Idempotent.
    fn shutdown(&self);
}

/// Worker-internal command channel.
enum WorkerCmd {
    Send(String),
    Shutdown,
}

/// Tungstenite-backed relay client. Owns one persistent connection on a
/// dedicated worker thread.
pub struct TungsteniteRelayClient {
    tx: Sender<WorkerCmd>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl TungsteniteRelayClient {
    /// Connect synchronously to `url` and spawn the read/write loop.
    /// Returns once the WebSocket handshake completes (or fails).
    pub fn connect(url: &str, on_event: EventCallback) -> Result<Self, RelayError> {
        install_rustls_provider();
        let (mut socket, _resp) =
            tungstenite::connect(url).map_err(|e| RelayError::Connect(format!("{url}: {e}")))?;
        set_read_timeout(&mut socket, Duration::from_millis(100));

        let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCmd>();
        let join = thread::spawn(move || run_worker(socket, cmd_rx, on_event));

        Ok(Self {
            tx: cmd_tx,
            join: Mutex::new(Some(join)),
        })
    }
}

impl RelayClient for TungsteniteRelayClient {
    fn send(&self, frame: String) -> Result<(), RelayError> {
        self.tx
            .send(WorkerCmd::Send(frame))
            .map_err(|_| RelayError::Disconnected)
    }

    fn shutdown(&self) {
        let _ = self.tx.send(WorkerCmd::Shutdown);
        if let Ok(mut guard) = self.join.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for TungsteniteRelayClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl std::fmt::Debug for TungsteniteRelayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TungsteniteRelayClient")
            .finish_non_exhaustive()
    }
}

fn run_worker(
    mut socket: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    cmd_rx: Receiver<WorkerCmd>,
    on_event: EventCallback,
) {
    loop {
        // Drain pending writes (non-blocking).
        let mut shutdown = false;
        loop {
            match cmd_rx.recv_timeout(Duration::from_millis(0)) {
                Ok(WorkerCmd::Send(frame)) => {
                    if let Err(e) = socket.send(tungstenite::Message::Text(frame)) {
                        eprintln!("nmp-signer-broker: relay write failed: {e}");
                        let _ = socket.close(None);
                        return;
                    }
                }
                Ok(WorkerCmd::Shutdown) => {
                    shutdown = true;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    shutdown = true;
                    break;
                }
            }
        }
        if shutdown {
            let _ = socket.close(None);
            return;
        }

        // Read one frame (short timeout so we cycle back to write drain).
        match socket.read() {
            Ok(tungstenite::Message::Text(text)) => {
                if let Some(value) = parse_event_frame(&text) {
                    on_event(value);
                }
            }
            Ok(tungstenite::Message::Binary(_)) => {
                // Ignore binary frames; NIP-01 is text-only.
            }
            Ok(tungstenite::Message::Ping(payload)) => {
                if let Err(e) = socket.send(tungstenite::Message::Pong(payload)) {
                    eprintln!("nmp-signer-broker: pong write failed: {e}");
                    return;
                }
            }
            Ok(tungstenite::Message::Pong(_)) | Ok(tungstenite::Message::Frame(_)) => {}
            Ok(tungstenite::Message::Close(_)) => {
                return;
            }
            Err(tungstenite::Error::Io(io_err))
                if matches!(
                    io_err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // Cycle back and check for outgoing writes.
                continue;
            }
            Err(e) => {
                eprintln!("nmp-signer-broker: relay read failed: {e}");
                return;
            }
        }
    }
}

/// Parse `["EVENT", <sub_id>, <event_json>]` and return the `<event_json>`
/// value. Other frame types return `None`.
fn parse_event_frame(text: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.len() < 3 {
        return None;
    }
    if arr.first()?.as_str()? != "EVENT" {
        return None;
    }
    Some(arr[2].clone())
}

/// Install the ring crypto provider for rustls. Mirrors `relay_worker.rs`.
fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn set_read_timeout(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    duration: Duration,
) {
    match socket.get_mut() {
        tungstenite::stream::MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(Some(duration));
        }
        tungstenite::stream::MaybeTlsStream::Rustls(stream) => {
            let tcp = stream.get_ref();
            let _ = tcp.set_read_timeout(Some(duration));
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_event_frame_extracts_inner_event_json() {
        let frame = r#"["EVENT","subA",{"id":"abc","kind":24133,"content":"x"}]"#;
        let v = parse_event_frame(frame).expect("event frame parses");
        assert_eq!(v.get("id").and_then(|x| x.as_str()), Some("abc"));
    }

    #[test]
    fn parse_event_frame_rejects_non_event_frames() {
        assert!(parse_event_frame(r#"["EOSE","subA"]"#).is_none());
        assert!(parse_event_frame(r#"["NOTICE","go away"]"#).is_none());
        assert!(parse_event_frame(r#"not json"#).is_none());
        assert!(parse_event_frame(r#"["EVENT"]"#).is_none());
    }
}
