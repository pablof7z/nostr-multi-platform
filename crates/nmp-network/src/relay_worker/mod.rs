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

// Re-export BackoffClass so callers (Pool, tests) can name the type without
// reaching into relay_protocol directly.
pub(crate) use crate::relay_protocol::BackoffClass;

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
    /// V-58 — one-shot backoff hint for the next reconnect. The worker
    /// stores the hint, applies it on the very next
    /// `RelayWorkerResult::Reconnect` branch, and then clears it so
    /// subsequent reconnects resume the normal exponential curve. Sending
    /// multiple hints before a disconnect: the last one wins.
    SetBackoffHint(BackoffClass),
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
// V-58: add RELAY_RECONNECT_DELAY_RATE_LIMITED for the rate-limited long-backoff.
use crate::relay_protocol::{
    is_permanent_error, jittered_backoff, RELAY_RECONNECT_DELAY_INITIAL,
    RELAY_RECONNECT_DELAY_MAX, RELAY_RECONNECT_DELAY_RATE_LIMITED,
};

/// After a relay has been connected for this duration, the reconnect backoff
/// is reset to [`RELAY_RECONNECT_DELAY_INITIAL`] on the next disconnect,
/// preventing accumulated backoff from earlier failure cycles (V-92 / GH #615).
const RELAY_BACKOFF_RESET_AFTER_SECS: Duration = Duration::from_secs(300); // 5 minutes

/// Spawn-with-explicit-keepalive worker that dials `relay_url` on
/// transport lane `role`.
///
/// T105: the worker dials the explicit URL (the resolved write/read relay),
/// not `role.url()`. `role` is retained as the diagnostic lane label so the
/// kernel keeps per-lane `RelayHealth` rows while the actual sockets multiply
/// per resolved URL.
///
/// Phase F (step 8): the prior `spawn_relay_worker` thin wrapper that passed
/// the 30s/30s production keepalive constants is gone — every caller (the
/// `pool::Pool` translator + the in-crate tests) reaches the worker through
/// this entry-point so the keepalive interval is always an explicit choice
/// at the call site. The production constants live in
/// [`crate::relay_protocol`].
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
    // V-58 — one-shot backoff hint. `None` = normal exponential curve.
    // Set to `Some(BackoffClass::RateLimited)` by a `SetBackoffHint` command
    // delivered while the socket is live; consumed (and cleared) in the very
    // next `Reconnect` branch so subsequent reconnects resume the normal curve.
    let mut backoff_hint: Option<BackoffClass> = None;
    let control = io_ready::spawn_control_inbox(control_rx);
    loop {
        match open_relay_socket(&relay_url) {
            Ok(mut socket) => {
                // Record when this connection attempt succeeded, for backoff reset
                // logic (V-92 / GH #615).
                let connected_at = Instant::now();
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
                let result = run_connected_relay(
                    role,
                    &relay_url,
                    generation,
                    &relay_tx,
                    &control,
                    &mut pending,
                    &mut socket,
                    &mut keepalive,
                    &mut backoff_hint,
                );
                match result {
                    RelayWorkerResult::Reconnect => {
                        // V-58: consume the one-shot hint (if any) to decide
                        // the reconnect delay for *this* disconnect.  A
                        // `RateLimited` hint overrides both the V-92 healthy-
                        // session reset and the normal exponential advance,
                        // so the worker backs off long even if the session was
                        // otherwise stable.
                        let delay = match backoff_hint.take() {
                            Some(BackoffClass::RateLimited) => {
                                // Pin backoff to the long value so *future*
                                // reconnects (hint absent) also start higher.
                                backoff = RELAY_RECONNECT_DELAY_RATE_LIMITED;
                                jittered_backoff(RELAY_RECONNECT_DELAY_RATE_LIMITED, &relay_url)
                            }
                            None | Some(BackoffClass::Transient) => {
                                // V-92 / GH #615: if the connection was healthy
                                // for at least RELAY_BACKOFF_RESET_AFTER_SECS,
                                // reset backoff to the initial value. This
                                // prevents accumulated backoff from much earlier
                                // failure cycles from affecting a relay that
                                // reconnects after sustained stable operation.
                                if connected_at.elapsed() >= RELAY_BACKOFF_RESET_AFTER_SECS {
                                    backoff = RELAY_RECONNECT_DELAY_INITIAL;
                                } else {
                                    // Mid-session drop: wait with backoff before
                                    // retrying. Do NOT reset backoff here — a
                                    // relay that connects and immediately
                                    // disconnects should back off progressively.
                                    backoff = (backoff * 2).min(RELAY_RECONNECT_DELAY_MAX);
                                }
                                jittered_backoff(backoff, &relay_url)
                            }
                        };
                        // T116c / G12: jitter already applied inside `delay`.
                        if !wait_before_reconnect(&control, &mut pending, delay) {
                            return;
                        }
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
                // Connect-time failure: no hint consumption — the hint was set
                // while a *previous* session was live; we apply the normal curve
                // at connect time (the hint is only meaningful after a live session
                // drops). The hint persists and will be consumed on the next
                // mid-session disconnect.
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
    backoff_hint: &mut Option<BackoffClass>,
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
        match control.drain_pending(pending, backoff_hint) {
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
            // V-58: hints arriving during the reconnect wait are silently
            // discarded — they arrived too late to influence the current
            // backoff (already computed above). The hint is not stored for
            // the *next* session either; the kernel re-sends a fresh hint on
            // the next rate-limited CLOSED after the socket reopens.
            Ok(RelayCommand::SetBackoffHint(_)) => {}
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
