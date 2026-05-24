use crate::keepalive::{KeepaliveAction, KeepaliveState};
use crate::role::RelayRole;
use std::collections::VecDeque;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

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
pub enum RelayEvent {
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
    pub fn role(&self) -> RelayRole {
        match self {
            Self::Connected { role, .. }
            | Self::Failed { role, .. }
            | Self::Closed { role, .. }
            | Self::Message { role, .. } => *role,
        }
    }

    /// The URL of the relay this event originated on (T105 routing key).
    pub fn relay_url(&self) -> &str {
        match self {
            Self::Connected { relay_url, .. }
            | Self::Failed { relay_url, .. }
            | Self::Closed { relay_url, .. }
            | Self::Message { relay_url, .. } => relay_url,
        }
    }

    pub fn generation(&self) -> u64 {
        match self {
            Self::Connected { generation, .. }
            | Self::Failed { generation, .. }
            | Self::Closed { generation, .. }
            | Self::Message { generation, .. } => *generation,
        }
    }
}

pub enum RelayCommand {
    Send(String),
    Shutdown,
}

enum RelayWorkerResult {
    Reconnect,
    PermanentFailure,
    Shutdown,
}

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

// V-01 Stage 3: backoff/keepalive constants and helpers now live in the
// always-compiled `relay_protocol` module so the wasm32 `BrowserRelayDriver`
// can reuse them. Behaviour and values are unchanged — these `use` statements
// preserve the legacy in-module names so the body of `run_relay_worker` is
// untouched.
use crate::relay_protocol::{
    is_permanent_error, jittered_backoff, KEEPALIVE_IDLE_THRESHOLD, KEEPALIVE_PONG_TIMEOUT,
    RELAY_RECONNECT_DELAY_INITIAL, RELAY_RECONNECT_DELAY_MAX,
};

/// Spawn a worker that dials `relay_url` on transport lane `role`.
///
/// T105: the worker dials the explicit URL (the resolved write/read relay),
/// not `role.url()`. `role` is retained as the diagnostic lane label so the
/// kernel keeps per-lane `RelayHealth` rows while the actual sockets multiply
/// per resolved URL.
///
/// T120b: production calls into [`spawn_relay_worker_with_keepalive`] with the
/// 30s/30s production constants; tests pass shorter intervals for hermetic
/// keepalive exercises.
pub fn spawn_relay_worker(
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
pub fn spawn_relay_worker_with_keepalive(
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
        );
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
                    RelayWorkerResult::PermanentFailure | RelayWorkerResult::Shutdown => return,
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
            Err(RecvTimeoutError::Timeout) => {}
            Ok(RelayCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => return false,
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
