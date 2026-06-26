use crate::keepalive::{KeepaliveAction, KeepaliveState};
use crate::role::RelayRole;
use std::collections::VecDeque;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

mod connect;
mod io_ready;
#[cfg(test)]
mod jitter_tests;
#[cfg(test)]
mod no_polling_tests;
mod socket_io;
#[cfg(test)]
mod tests;

use connect::open_relay_socket;
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
    // The `relay_url()` / `generation()` accessors were removed when the pool
    // translator was reworked to pre-translate events off-lock: the variants
    // are now destructured directly into a `PreparedEvent` in `pool::inner`,
    // so the by-reference routing-key accessors had no remaining caller.
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
    /// Reconnect-preamble registration (REQ-before-EVENT fix).
    ///
    /// Registers a list of raw text frames that the worker will inject at
    /// the FRONT of its outbound `pending` queue immediately after emitting
    /// `RelayEvent::Connected`, BEFORE `run_connected_relay` drains any
    /// actor-posted `Send` commands.  This structural injection guarantees
    /// that a REQ subscription frame is always written to the wire before
    /// any EVENT command that the actor posts from its `Opened` hook —
    /// even when a sign EVENT is queued while the socket is reconnecting.
    ///
    /// The last write wins (whole list replaces the prior preamble).
    /// The preamble survives every reconnect so that repeated socket resets
    /// keep the guarantee without the actor needing to re-register.
    ///
    /// D0-clean: the frames are opaque `String`s; no protocol noun crosses
    /// the `nmp-network` boundary.
    SetReconnectPreamble(Vec<String>),
}

enum RelayWorkerResult {
    Reconnect,
    PermanentFailure,
    Shutdown,
}

pub(super) type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

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
    user_agent: Option<String>,
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
            user_agent,
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
    user_agent: Option<String>,
) {
    let mut pending = VecDeque::new();
    // Reconnect preamble: frames injected at the FRONT of `pending` on every
    // connect, BEFORE the actor's Opened hook can enqueue outbound commands.
    // This is the structural REQ-before-EVENT guarantee for NIP-46 (and any
    // other subscriber that needs its REQ sent before the relay can deliver
    // matching EVENTs).  Set via `SetReconnectPreamble`; survives reconnects.
    let mut preamble: Vec<String> = Vec::new();
    let mut backoff = RELAY_RECONNECT_DELAY_INITIAL;
    // V-58 — one-shot backoff hint. `None` = normal exponential curve.
    // Set by a `SetBackoffHint` command delivered while the socket is live;
    // consumed (and cleared via `.take()`) in the first `Reconnect` branch
    // so subsequent reconnects resume the normal curve. Lifetime is bounded
    // to the next disconnect after the hint arrives — a hint set during a
    // healthy session that stays up a long time will still apply to whatever
    // drop eventually terminates it. This is intentional: a relay that said
    // "rate-limited" continues to warrant long backoff on its next reconnect
    // regardless of how long it stayed healthy after the CLOSED.
    let mut backoff_hint: Option<BackoffClass> = None;
    let control = io_ready::spawn_control_inbox(control_rx);
    loop {
        match open_relay_socket(&relay_url, user_agent.as_deref()) {
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
                // REQ-before-EVENT: inject the registered preamble at the FRONT
                // of the outbound queue immediately after signalling Connected.
                // The actor's Opened hook fires on the other side of the channel
                // and may post Send commands; because those arrive via
                // `drain_pending` inside `run_connected_relay`, they land AFTER
                // the preamble frames already in `pending`.
                for f in preamble.iter().rev() {
                    pending.push_front(f.clone());
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
                    &mut preamble,
                );
                match result {
                    RelayWorkerResult::Reconnect => {
                        // V-58 / V-92: delegate the schedule decision to the
                        // extracted `apply_reconnect_backoff` so the logic is
                        // testable without a live socket. The hint is one-shot:
                        // `.take()` clears it so subsequent reconnects resume
                        // the normal exponential curve.
                        let base = apply_reconnect_backoff(
                            backoff_hint.take(),
                            &mut backoff,
                            connected_at.elapsed(),
                        );
                        // T116c / G12: jitter spreads simultaneous reconnects.
                        let delay = jittered_backoff(base, &relay_url);
                        if !wait_before_reconnect(&control, &mut pending, &mut preamble, delay) {
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
                // Connect-time failure (never reached run_connected_relay, so
                // no hint was ever stored in backoff_hint for this iteration).
                // Apply the normal exponential curve; no hint to consume.
                // T116c / G12: jitter spreads simultaneous reconnects
                // across a [0, 5s] window to avoid global thundering-herd.
                if !wait_before_reconnect(
                    &control,
                    &mut pending,
                    &mut preamble,
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
    preamble: &mut Vec<String>,
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
        match control.drain_pending(pending, backoff_hint, preamble) {
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
            io_ready::ControlDrain::Disconnected => {
                // The control sender was dropped (caller tore down the Pool slot without
                // sending Shutdown). Emit an honest Closed event so consumers tracking
                // slot health see a terminal state rather than being left at the last
                // non-terminal event forever.
                let _ = relay_tx.send(RelayEvent::Closed {
                    role,
                    relay_url: relay_url.to_string(),
                    generation,
                });
                return RelayWorkerResult::Shutdown;
            }
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
                // T120b: flush the ping frame. The pong-timeout clock starts
                // only when the ping actually reaches the wire — not when the
                // FSM decides to emit it. `on_ping_flushed` stamps the timer;
                // if the write is Blocked the timer stays unset and the FSM
                // will return EmitPing again on the next tick, retrying the
                // write without advancing the pong-timeout window.
                //
                // INVARIANT: only OR-in `true` here; never unconditionally
                // set `wants_write = false`. If `flush_relay_writes` above
                // returned Blocked (wants_write is already true because
                // `pending` is non-empty), the ping's own flush succeeding
                // must not clear write interest for the still-pending frames.
                match flush_socket_message(socket, Message::Ping(Vec::new())) {
                    FlushResult::Flushed => {
                        keepalive.on_ping_flushed(Instant::now());
                        // Do not reset wants_write to false — pending frames
                        // from flush_relay_writes may still need write interest.
                    }
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
        // On edge-triggered platforms (kqueue on macOS uses EV_CLEAR), a readable
        // event that arrives in the same kevent() batch as a control or writable
        // event is not re-delivered on the next poll call — the edge transition was
        // already consumed. Drain reads unconditionally before looping back for
        // command/write processing so an EOF or inbound frame is never silently
        // dropped when a waker fires simultaneously with a socket readable signal.
        if ready.readable {
            if let Some(result) =
                drain_relay_reads(role, relay_url, generation, relay_tx, socket, keepalive)
            {
                return result;
            }
        }
        if ready.control || ready.writable {
            continue;
        }
    }
}

/// V-58 / V-92 — compute and apply the reconnect backoff for one disconnect.
///
/// Called by `run_relay_worker` in the `Reconnect` branch. Mutates
/// `current_backoff` in place (so the next call starts from the updated value)
/// and returns the jittered delay to wait before retrying.
///
/// Rules (in priority order):
///
/// 1. `hint = Some(BackoffClass::RateLimited)` — relay just rate-limited us.
///    Override V-92 reset and normal exponential advance: pin the base to
///    [`RELAY_RECONNECT_DELAY_RATE_LIMITED`] (60 s) regardless of session age.
///    Jitter is applied on top.
/// 2. `hint = None | Some(BackoffClass::Transient)` — normal transient drop.
///    V-92 / GH #615: if the session ran for ≥ [`RELAY_BACKOFF_RESET_AFTER_SECS`]
///    (5 min), reset the base to [`RELAY_RECONNECT_DELAY_INITIAL`] so
///    accumulated earlier-cycle debt does not bleed into stable relays.
///    Otherwise advance the exponential curve (×2 capped at
///    [`RELAY_RECONNECT_DELAY_MAX`]). Jitter is applied on top.
///
/// This is `pub(crate)` so tests can call the real production logic directly
/// without spinning up a socket.
pub(crate) fn apply_reconnect_backoff(
    hint: Option<BackoffClass>,
    current_backoff: &mut Duration,
    connected_elapsed: Duration,
) -> Duration {
    match hint {
        Some(BackoffClass::RateLimited) => {
            // V-58: pin base to the long value so *future* reconnects
            // (hint absent) also start from the rate-limited floor.
            *current_backoff = RELAY_RECONNECT_DELAY_RATE_LIMITED;
            *current_backoff
        }
        None | Some(BackoffClass::Transient) => {
            // V-92 / GH #615: healthy session → reset to initial.
            if connected_elapsed >= RELAY_BACKOFF_RESET_AFTER_SECS {
                *current_backoff = RELAY_RECONNECT_DELAY_INITIAL;
            } else {
                // Mid-session drop: advance the exponential curve.
                *current_backoff = (*current_backoff * 2).min(RELAY_RECONNECT_DELAY_MAX);
            }
            *current_backoff
        }
    }
}

/// Wait for the reconnect delay to elapse, buffering incoming `Send` commands
/// and updating the `preamble` if `SetReconnectPreamble` arrives.
///
/// **Guardrail 1 (codex):** `SetReconnectPreamble` arriving during the wait is
/// STORED (not discarded), unlike `SetBackoffHint`.  A fast-flap scenario where
/// the actor registers the preamble while the socket is between reconnects would
/// silently drop it if we discarded it here, leaving the NEXT reconnect without
/// the REQ guarantee.
fn wait_before_reconnect(
    control: &io_ready::ControlInbox,
    pending: &mut VecDeque<String>,
    preamble: &mut Vec<String>,
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
            // Guardrail 1: store the preamble — do NOT discard it.
            Ok(RelayCommand::SetReconnectPreamble(frames)) => *preamble = frames,
            Err(RecvTimeoutError::Timeout) => {}
            Ok(RelayCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}
