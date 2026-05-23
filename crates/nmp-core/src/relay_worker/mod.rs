use crate::keepalive::{KeepaliveAction, KeepaliveState};
use crate::kernel::RelayFrame;
use crate::relay::RelayRole;
use std::collections::VecDeque;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

/// Convert a native [`tungstenite::Message`] into the wire-transport-agnostic
/// [`RelayFrame`] consumed by `Kernel::handle_message`.
///
/// V-01 Phase 1c: the kernel no longer names `tungstenite`, so the conversion
/// happens here (the only native-feature site that owns both vocabularies).
/// `Message::Frame` (raw-frame) maps to [`RelayFrame::Binary`] — the kernel's
/// only observable for non-text payloads is the bytes counter.
pub(crate) fn tungstenite_message_to_relay_frame(message: Message) -> RelayFrame {
    match message {
        Message::Text(text) => RelayFrame::Text(text),
        Message::Binary(bytes) => RelayFrame::Binary(bytes),
        Message::Ping(_) => RelayFrame::Ping,
        Message::Pong(_) => RelayFrame::Pong,
        Message::Close(frame) => RelayFrame::Close(frame.map(|f| f.reason.to_string())),
        Message::Frame(_) => RelayFrame::Binary(Vec::new()),
    }
}

mod io_ready;
#[cfg(test)]
mod no_polling_tests;
mod socket_io;
#[cfg(test)]
mod tests;

use socket_io::{drain_relay_reads, flush_relay_writes, flush_socket_message, FlushResult};

/// One physical relay-worker event.
///
/// T105: every event carries both the diagnostic `role` (the lane this URL
/// belongs to — Content/Indexer) AND the actual `relay_url` the socket
/// connects to. The url is what the URL-keyed `relay_controls` map indexes
/// on; the role is what the kernel's per-lane diagnostics use.
pub(crate) enum RelayEvent {
    Connected {
        role: RelayRole,
        relay_url: String,
        generation: u64,
    },
    Failed {
        role: RelayRole,
        relay_url: String,
        generation: u64,
        error: String,
    },
    Closed {
        role: RelayRole,
        relay_url: String,
        generation: u64,
    },
    Message {
        role: RelayRole,
        relay_url: String,
        generation: u64,
        message: Message,
    },
}

impl RelayEvent {
    #[allow(dead_code)] // Used by ingest dispatch; kept for diagnostic helpers.
    pub(crate) fn role(&self) -> RelayRole {
        match self {
            Self::Connected { role, .. }
            | Self::Failed { role, .. }
            | Self::Closed { role, .. }
            | Self::Message { role, .. } => *role,
        }
    }

    /// The URL of the relay this event originated on (T105 routing key).
    pub(crate) fn relay_url(&self) -> &str {
        match self {
            Self::Connected { relay_url, .. }
            | Self::Failed { relay_url, .. }
            | Self::Closed { relay_url, .. }
            | Self::Message { relay_url, .. } => relay_url,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        match self {
            Self::Connected { generation, .. }
            | Self::Failed { generation, .. }
            | Self::Closed { generation, .. }
            | Self::Message { generation, .. } => *generation,
        }
    }
}

pub(crate) enum RelayCommand {
    Send(String),
    Shutdown,
}

enum RelayWorkerResult {
    Reconnect,
    PermanentFailure,
    Shutdown,
}

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;
/// Initial mid-session reconnect delay. Doubled on each consecutive failure
/// up to [`RELAY_RECONNECT_DELAY_MAX`]; reset to this value on a successful
/// connect.
const RELAY_RECONNECT_DELAY_INITIAL: Duration = Duration::from_secs(3);
const RELAY_RECONNECT_DELAY_MAX: Duration = Duration::from_secs(300);
/// T120b / G4 — emit a Ping after this much inbound silence.
const KEEPALIVE_IDLE_THRESHOLD: Duration = Duration::from_secs(30);
/// T120b / G4 — declare the socket dead if no inbound frame arrives within
/// this window after a Ping is emitted.
const KEEPALIVE_PONG_TIMEOUT: Duration = Duration::from_secs(30);

/// T116c / G12 — per-URL deterministic jitter to prevent thundering-herd
/// reconnects when many relays fail simultaneously (e.g. network partition
/// recovery). Uses a hash of the URL bytes to produce a spread that is:
///   - deterministic per URL (same URL always gets the same jitter offset),
///   - spread across all active relays (different URLs → different offsets),
///   - bounded to [0, 5s] so worst-case individual delay is `base + 5s`.
///
/// No shared state needed: each worker computes its own jitter independently.
pub(crate) fn jittered_backoff(base: Duration, url: &str) -> Duration {
    let hash = url
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    let jitter_ms = hash % 5000; // 0–4999 ms spread
    base + Duration::from_millis(jitter_ms)
}

/// HTTP-level denial: the relay explicitly rejected the connection.
/// 401 and 403 are both permanent until the user changes credentials/policy.
fn is_permanent_error(error: &str) -> bool {
    error.contains("403") || error.contains("401") || error.contains("Forbidden")
}

/// Spawn a worker that dials `relay_url` on transport lane `role`.
///
/// T105: the worker dials the explicit URL (the resolved write/read relay),
/// not `role.url()`. `role` is retained as the diagnostic lane label so the
/// kernel keeps per-lane RelayHealth rows while the actual sockets multiply
/// per resolved URL.
///
/// T120b: production calls into [`spawn_relay_worker_with_keepalive`] with the
/// 30s/30s production constants; tests pass shorter intervals for hermetic
/// keepalive exercises.
pub(crate) fn spawn_relay_worker(
    role: RelayRole,
    relay_url: String,
    generation: u64,
    relay_tx: Sender<RelayEvent>,
) -> Sender<RelayCommand> {
    spawn_relay_worker_with_keepalive(
        role,
        relay_url,
        generation,
        relay_tx,
        KEEPALIVE_IDLE_THRESHOLD,
        KEEPALIVE_PONG_TIMEOUT,
    )
}

/// Spawn-with-explicit-keepalive variant. The production entry-point
/// [`spawn_relay_worker`] is a thin wrapper passing the 30s/30s constants;
/// tests use this directly to exercise the keepalive path on millisecond
/// budgets without 30s sleeps.
pub(crate) fn spawn_relay_worker_with_keepalive(
    role: RelayRole,
    relay_url: String,
    generation: u64,
    relay_tx: Sender<RelayEvent>,
    keepalive_idle: Duration,
    keepalive_pong_timeout: Duration,
) -> Sender<RelayCommand> {
    let (control_tx, control_rx) = mpsc::channel();
    thread::spawn(move || {
        run_relay_worker(
            role,
            relay_url,
            generation,
            relay_tx,
            control_rx,
            keepalive_idle,
            keepalive_pong_timeout,
        )
    });
    control_tx
}

fn run_relay_worker(
    role: RelayRole,
    relay_url: String,
    generation: u64,
    relay_tx: Sender<RelayEvent>,
    control_rx: Receiver<RelayCommand>,
    keepalive_idle: Duration,
    keepalive_pong_timeout: Duration,
) {
    let mut pending = VecDeque::new();
    let mut backoff = RELAY_RECONNECT_DELAY_INITIAL;
    let control = io_ready::spawn_control_inbox(control_rx);
    loop {
        match open_relay_socket(&relay_url) {
            Ok(mut socket) => {
                if relay_tx
                    .send(RelayEvent::Connected {
                        role,
                        relay_url: relay_url.clone(),
                        generation,
                    })
                    .is_err()
                {
                    return;
                }
                // T120b: fresh socket → fresh keepalive driver. `Instant::now()`
                // is the moment the socket actually opened; the first
                // `keepalive_idle` of silence is tolerated.
                let mut keepalive =
                    KeepaliveState::new(Instant::now(), keepalive_idle, keepalive_pong_timeout);
                match run_connected_relay(
                    role,
                    &relay_url,
                    generation,
                    &relay_tx,
                    &control,
                    &mut pending,
                    &mut socket,
                    &mut keepalive,
                ) {
                    RelayWorkerResult::Reconnect => {
                        // Mid-session drop: wait with backoff before retrying.
                        // Do NOT reset backoff here — a relay that connects and
                        // immediately disconnects should back off progressively,
                        // not spin at 3 s per cycle.
                        // T116c / G12: jitter spreads simultaneous reconnects
                        // across a [0, 5s] window to avoid global thundering-herd.
                        if !wait_before_reconnect(
                            &control,
                            &mut pending,
                            jittered_backoff(backoff, &relay_url),
                        ) {
                            return;
                        }
                        backoff = (backoff * 2).min(RELAY_RECONNECT_DELAY_MAX);
                    }
                    // HTTP 401/403 received mid-session (e.g., after NIP-42 auth
                    // failure): relay is denying this client permanently.
                    RelayWorkerResult::PermanentFailure => return,
                    RelayWorkerResult::Shutdown => return,
                }
            }
            Err(error) => {
                // HTTP 403/401 at connect time = relay denies this client;
                // no point reconnecting.
                let permanent = is_permanent_error(&error);
                let _ = relay_tx.send(RelayEvent::Failed {
                    role,
                    relay_url: relay_url.clone(),
                    generation,
                    error,
                });
                if permanent {
                    return;
                }
                // T116c / G12: jitter spreads simultaneous reconnects
                // across a [0, 5s] window to avoid global thundering-herd.
                if !wait_before_reconnect(
                    &control,
                    &mut pending,
                    jittered_backoff(backoff, &relay_url),
                ) {
                    return;
                }
                backoff = (backoff * 2).min(RELAY_RECONNECT_DELAY_MAX);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_connected_relay(
    role: RelayRole,
    relay_url: &str,
    generation: u64,
    relay_tx: &Sender<RelayEvent>,
    control: &io_ready::ControlInbox,
    pending: &mut VecDeque<String>,
    socket: &mut RelaySocket,
    keepalive: &mut KeepaliveState,
) -> RelayWorkerResult {
    let (mut poller, _wake_guard) = match io_ready::RelayPoller::new(socket, control) {
        Ok(poller) => poller,
        Err(error) => {
            let _ = relay_tx.send(RelayEvent::Failed {
                role,
                relay_url: relay_url.to_string(),
                generation,
                error: format!("relay readiness setup failed: {error}"),
            });
            return RelayWorkerResult::Reconnect;
        }
    };
    loop {
        match control.drain_pending(pending) {
            io_ready::ControlDrain::Continue => {}
            io_ready::ControlDrain::Shutdown => {
                let _ = socket.close(None);
                let _ = relay_tx.send(RelayEvent::Closed {
                    role,
                    relay_url: relay_url.to_string(),
                    generation,
                });
                return RelayWorkerResult::Shutdown;
            }
            io_ready::ControlDrain::Disconnected => return RelayWorkerResult::Shutdown,
        }

        let mut wants_write =
            match flush_relay_writes(role, relay_url, generation, relay_tx, pending, socket) {
                FlushResult::Flushed => false,
                FlushResult::Blocked => true,
                FlushResult::Reconnect => return RelayWorkerResult::Reconnect,
            };
        if let Err(error) = poller.set_wants_write(socket, wants_write) {
            let _ = relay_tx.send(RelayEvent::Failed {
                role,
                relay_url: relay_url.to_string(),
                generation,
                error: format!("relay readiness update failed: {error}"),
            });
            return RelayWorkerResult::Reconnect;
        }

        // T120b — drive keepalive from explicit readiness deadlines. The
        // worker blocks until the socket is ready, a control command wakes it,
        // or the keepalive FSM's next deadline arrives.
        match keepalive.step(Instant::now()) {
            KeepaliveAction::Idle => {}
            KeepaliveAction::EmitPing => {
                match flush_socket_message(socket, Message::Ping(Vec::new())) {
                    FlushResult::Flushed => wants_write = false,
                    FlushResult::Blocked => wants_write = true,
                    FlushResult::Reconnect => {
                        let _ = relay_tx.send(RelayEvent::Failed {
                            role,
                            relay_url: relay_url.to_string(),
                            generation,
                            error: "ping write failed".to_string(),
                        });
                        return RelayWorkerResult::Reconnect;
                    }
                }
            }
            KeepaliveAction::Dead => {
                let _ = relay_tx.send(RelayEvent::Failed {
                    role,
                    relay_url: relay_url.to_string(),
                    generation,
                    error: "keepalive timeout (no pong within 30s)".to_string(),
                });
                return RelayWorkerResult::Reconnect;
            }
        }

        if let Err(error) = poller.set_wants_write(socket, wants_write) {
            let _ = relay_tx.send(RelayEvent::Failed {
                role,
                relay_url: relay_url.to_string(),
                generation,
                error: format!("relay readiness update failed: {error}"),
            });
            return RelayWorkerResult::Reconnect;
        }

        let timeout = keepalive
            .next_deadline()
            .saturating_duration_since(Instant::now());
        let ready = match poller.wait(timeout) {
            Ok(ready) => ready,
            Err(error) => {
                let _ = relay_tx.send(RelayEvent::Failed {
                    role,
                    relay_url: relay_url.to_string(),
                    generation,
                    error: format!("relay readiness wait failed: {error}"),
                });
                return RelayWorkerResult::Reconnect;
            }
        };
        if ready.control || ready.writable {
            continue;
        }
        if ready.readable {
            if let Some(result) =
                drain_relay_reads(role, relay_url, generation, relay_tx, socket, keepalive)
            {
                return result;
            }
        }
    }
}

fn wait_before_reconnect(
    control: &io_ready::ControlInbox,
    pending: &mut VecDeque<String>,
    delay: Duration,
) -> bool {
    let deadline = Instant::now() + delay;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            return true;
        }
        match control.recv_timeout(remaining) {
            Ok(RelayCommand::Send(text)) => pending.push_back(text),
            Ok(RelayCommand::Shutdown) => return false,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn open_relay_socket(relay_url: &str) -> Result<RelaySocket, String> {
    install_rustls_provider();
    let (socket, _response) = connect(relay_url).map_err(|error| error.to_string())?;
    Ok(socket)
}

fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
