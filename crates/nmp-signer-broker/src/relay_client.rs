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
//! ## Threading model — V-13 / D8 compliance
//!
//! Polling is forbidden (`AGENTS.md` §"No polling — ever", doctrine D8).
//! The worker runs a single thread that blocks via [`mio::Poll`] on either:
//!   - socket readiness (the OS notifies us when bytes are available or the
//!     socket is writable again after a `WouldBlock`), or
//!   - a control-channel wakeup (a [`mio::Waker`] fired from the forwarder
//!     thread when a `WorkerCmd` arrives).
//!
//! This mirrors `nmp-core::relay_worker::io_ready`. We do NOT use
//! `set_read_timeout` + `try_recv()` in a loop — that pattern is banned
//! project-wide (see `crates/nmp-core/src/relay_worker/no_polling_tests.rs`).
//!
//! V-13 Stage 1 in `docs/BACKLOG.md` plans to extract a shared
//! `nmp-relay-conn` crate so this code and the native `relay_worker` share
//! the same primitive; that dedupe is intentionally deferred to a separate
//! PR to keep this change scoped.
//!
//! ## Reconnect — V-14
//!
//! `run_worker` does not exit on the first transport error. When the socket
//! closes or read/write fails, the worker waits with exponential backoff
//! (3 s → 6 s → … → 300 s, jittered per-URL via
//! [`nmp_core::relay_protocol::jittered_backoff`]) and reconnects
//! transparently. After each successful reconnect, all frames previously
//! installed via [`RelayClient::subscribe`] are replayed so the inbound
//! subscription survives a flap.
//!
//! Reconnect terminates only on:
//!   - explicit shutdown (`WorkerCmd::Shutdown` or trait `shutdown()`),
//!   - permanent HTTP-level denial (401/403, per
//!     [`nmp_core::relay_protocol::is_permanent_error`]).
//!
//! ## Trait surface
//!
//! [`RelayClient`] is a trait so unit tests can stub the transport without
//! spinning up a TCP listener. The production impl
//! [`TungsteniteRelayClient`] uses a blocking `tungstenite` socket on its
//! own thread.

use std::collections::VecDeque;
use std::io;
use std::net::TcpStream;
use std::os::unix::io::AsRawFd;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Once;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token, Waker};
use serde_json::Value;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;

use nmp_core::relay_protocol::{
    is_permanent_error, jittered_backoff, RELAY_RECONNECT_DELAY_INITIAL,
    RELAY_RECONNECT_DELAY_MAX,
};

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
            Self::Connect(m) => write!(f, "connect failed: {m}"),
            Self::Write(m) => write!(f, "write failed: {m}"),
            Self::Disconnected => f.write_str("relay client disconnected"),
        }
    }
}

impl std::error::Error for RelayError {}

/// Trait the broker programs against. Production: [`TungsteniteRelayClient`].
/// Tests: stub with a `Vec`-backed sink.
pub trait RelayClient: Send + Sync {
    /// Send a raw NIP-01 client frame (`["EVENT", ...]`, `["CLOSE", ...]`).
    /// Frames sent via this method are NOT replayed after a reconnect — use
    /// it for transient one-shot messages (RPC publishes, CLOSE).
    fn send(&self, frame: String) -> Result<(), RelayError>;

    /// Install a long-lived NIP-01 client frame (`["REQ", ...]`). The client
    /// sends the frame now AND remembers it so it can be replayed verbatim
    /// after every reconnect. This is what makes V-14 (auto-reconnect) end-
    /// to-end correct: a transparent re-connect that lost the inbound
    /// subscription would deliver no events.
    ///
    /// Default impl forwards to `send` for transports that have no concept
    /// of reconnect (e.g. test stubs).
    fn subscribe(&self, frame: String) -> Result<(), RelayError> {
        self.send(frame)
    }

    /// Cancel the worker, close the socket. Idempotent.
    fn shutdown(&self);
}

/// Worker-internal command channel.
enum WorkerCmd {
    /// Transient outbound frame; not replayed on reconnect.
    Send(String),
    /// Replay-on-reconnect frame; the worker also persists it in its
    /// `subscriptions` list so it survives across reconnects.
    Subscribe(String),
    Shutdown,
}

/// Tungstenite-backed relay client. Owns one persistent connection on a
/// dedicated worker thread.
pub struct TungsteniteRelayClient {
    tx: Sender<WorkerCmd>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl TungsteniteRelayClient {
    /// Connect synchronously to `url` and spawn the read/write loop. Returns
    /// once the initial WebSocket handshake completes (or fails). The worker
    /// then enters its readiness loop and auto-reconnects with backoff if
    /// the socket later drops (V-14).
    #[must_use]
    pub fn connect(url: &str, on_event: EventCallback) -> Result<Self, RelayError> {
        install_rustls_provider();
        let socket =
            open_socket(url).map_err(|e| RelayError::Connect(format!("{url}: {e}")))?;

        let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCmd>();
        let url_owned = url.to_string();
        let join = thread::spawn(move || run_worker(url_owned, socket, cmd_rx, on_event));

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

    fn subscribe(&self, frame: String) -> Result<(), RelayError> {
        self.tx
            .send(WorkerCmd::Subscribe(frame))
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

// ─── Worker internals ────────────────────────────────────────────────────────

type RelaySocket = tungstenite::WebSocket<MaybeTlsStream<TcpStream>>;

const SOCKET: Token = Token(0);
const CONTROL: Token = Token(1);

/// Why a connected session ended. Distinguishes recoverable drops from
/// terminal states so the supervisor knows when to back off and reconnect
/// vs. exit.
enum SessionEnd {
    /// Recoverable drop (read/write error, server close). Reconnect with
    /// backoff.
    Reconnect,
    /// Explicit shutdown via control channel.
    Shutdown,
    /// HTTP 401/403: relay has permanently denied this client. No point
    /// reconnecting; thread exits.
    PermanentFailure,
}

/// Supervisor loop: own one URL, reconnect transparently on drop, replay
/// subscriptions after each successful connect (V-14). Exits only on
/// explicit shutdown or a permanent HTTP denial.
fn run_worker(
    url: String,
    initial_socket: RelaySocket,
    cmd_rx: Receiver<WorkerCmd>,
    on_event: EventCallback,
) {
    let mut socket = Some(initial_socket);
    let mut backoff = RELAY_RECONNECT_DELAY_INITIAL;
    // Long-lived frames that must survive a reconnect (`REQ` envelopes
    // installed via `subscribe`). Owned here so we can replay them on the
    // worker thread without locking back to the public API surface.
    let mut subscriptions: Vec<String> = Vec::new();
    // Outbound queue for transient writes that arrived while we were
    // mid-flush / mid-reconnect; drained on each connected session.
    let mut pending: VecDeque<String> = VecDeque::new();
    // Spawn a forwarder so we can wake the mio poll on control input. The
    // primary `cmd_rx` is owned by the forwarder; the worker reads from the
    // forwarder's `local_rx` and is woken via `Waker`.
    let control = match spawn_control_inbox(cmd_rx) {
        Ok(c) => c,
        Err(_) => return,
    };

    loop {
        // Step 1 — open (or reuse) a connected socket. The first iteration
        // reuses `initial_socket`; subsequent ones dial fresh. Before each
        // fresh dial we drain queued commands so a Shutdown that arrived
        // during the previous backoff aborts us immediately, instead of
        // sitting in the channel until the OS-level connect timeout
        // returns. (The residual stall window — Shutdown arriving while
        // `tungstenite::connect()` is blocked mid-handshake — is shared
        // with `nmp-core::relay_worker` and is V-13 Stage 1 territory.)
        let mut connected = match socket.take() {
            Some(s) => s,
            None => {
                if drain_for_shutdown(&control, &mut pending, &mut subscriptions) {
                    return;
                }
                match open_socket(&url) {
                    Ok(s) => {
                        backoff = RELAY_RECONNECT_DELAY_INITIAL;
                        s
                    }
                    Err(err) => {
                        if is_permanent_error(&err) {
                            return;
                        }
                        // Wait with backoff before retrying.
                        // `wait_before_reconnect` blocks on the control
                        // channel so a Shutdown command wakes us promptly;
                        // transient sends arriving during the wait are
                        // queued via `pending`/`subscriptions`.
                        if !wait_before_reconnect(
                            &control,
                            &mut pending,
                            &mut subscriptions,
                            jittered_backoff(backoff, &url),
                        ) {
                            return;
                        }
                        backoff = (backoff * 2).min(RELAY_RECONNECT_DELAY_MAX);
                        continue;
                    }
                }
            }
        };

        // Step 2 — replay every installed subscription on the fresh socket
        // so the inbound REQ survives a reconnect (V-14).
        let mut replay_failed = false;
        for frame in &subscriptions {
            if connected.send(Message::Text(frame.clone())).is_err() {
                replay_failed = true;
                break;
            }
        }
        if replay_failed {
            let _ = connected.close(None);
            if !wait_before_reconnect(
                &control,
                &mut pending,
                &mut subscriptions,
                jittered_backoff(backoff, &url),
            ) {
                return;
            }
            backoff = (backoff * 2).min(RELAY_RECONNECT_DELAY_MAX);
            continue;
        }

        // Step 3 — run the connected session until it ends.
        let end = run_connected(
            &mut connected,
            &control,
            &on_event,
            &mut pending,
            &mut subscriptions,
        );
        let _ = connected.close(None);
        match end {
            SessionEnd::Shutdown | SessionEnd::PermanentFailure => return,
            SessionEnd::Reconnect => {
                // Do NOT reset backoff here — a relay that connects and
                // immediately drops should back off progressively, not spin
                // at the initial delay (mirrors `nmp-core::relay_worker`
                // policy).
                if !wait_before_reconnect(
                    &control,
                    &mut pending,
                    &mut subscriptions,
                    jittered_backoff(backoff, &url),
                ) {
                    return;
                }
                backoff = (backoff * 2).min(RELAY_RECONNECT_DELAY_MAX);
            }
        }
    }
}

/// Drive one connected session. Blocks on readiness (socket or control
/// wakeup) — no polling. Returns when the session ends.
fn run_connected(
    socket: &mut RelaySocket,
    control: &ControlInbox,
    on_event: &EventCallback,
    pending: &mut VecDeque<String>,
    subscriptions: &mut Vec<String>,
) -> SessionEnd {
    // Switch the underlying TCP to non-blocking so `socket.read()` /
    // `socket.write()` return `WouldBlock` instead of stalling; readiness
    // is provided by `mio::Poll`.
    if set_nonblocking(socket, true).is_err() {
        return SessionEnd::Reconnect;
    }

    let mut poll = match Poll::new() {
        Ok(p) => p,
        Err(_) => return SessionEnd::Reconnect,
    };
    let waker = match Waker::new(poll.registry(), CONTROL) {
        Ok(w) => w,
        Err(_) => return SessionEnd::Reconnect,
    };
    let _wake_guard = control.install_waker(waker);
    let mut wants_write = false;
    if register_socket(&poll, socket, wants_write, false).is_err() {
        return SessionEnd::Reconnect;
    }
    let mut events = Events::with_capacity(16);

    loop {
        // Drain pending control commands without blocking. `try_recv` is
        // allowed here because it is inside the readiness helper (see the
        // doctrine carve-out at `nmp-core::relay_worker::no_polling_tests`).
        match control.drain(pending, subscriptions, socket) {
            ControlDrain::Continue => {}
            ControlDrain::Shutdown => return SessionEnd::Shutdown,
            ControlDrain::Disconnected => return SessionEnd::Shutdown,
            ControlDrain::WriteFailed => return SessionEnd::Reconnect,
        }

        // Flush any queued outbound text frames; mark `wants_write` if the
        // socket is back-pressured.
        let next_wants_write = match flush_pending(socket, pending) {
            FlushResult::Flushed => false,
            FlushResult::Blocked => true,
            FlushResult::Reconnect => return SessionEnd::Reconnect,
        };
        if next_wants_write != wants_write {
            if register_socket(&poll, socket, next_wants_write, true).is_err() {
                return SessionEnd::Reconnect;
            }
            wants_write = next_wants_write;
        }

        // Block until the socket is readable/writable OR a control wakeup
        // fires. No timeout — purely event-driven (D8). If the relay goes
        // completely silent for hours we'll sit here on a syscall, which is
        // exactly what we want; reconnect/keepalive policy belongs to a
        // higher layer (deferred to V-13 Stage 1 shared crate).
        if let Err(err) = poll.poll(&mut events, None) {
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return SessionEnd::Reconnect;
        }

        let mut socket_readable = false;
        let mut socket_writable = false;
        for event in &events {
            match event.token() {
                CONTROL => {
                    // Loop back to the top to drain commands.
                }
                SOCKET => {
                    socket_readable |= event.is_readable();
                    socket_writable |= event.is_writable();
                }
                _ => {}
            }
        }

        if socket_writable {
            // Loop back; the next iteration's flush will progress.
            continue;
        }
        if socket_readable {
            match drain_reads(socket, on_event) {
                ReadResult::Continue => {}
                ReadResult::Closed => return SessionEnd::Reconnect,
                ReadResult::Permanent => return SessionEnd::PermanentFailure,
            }
        }
    }
}

// ─── Control channel — mio-wakeable command inbox ────────────────────────────

struct ControlInbox {
    rx: Receiver<WorkerCmd>,
    wake: Arc<Mutex<Option<Waker>>>,
}

enum ControlDrain {
    Continue,
    Shutdown,
    Disconnected,
    WriteFailed,
}

fn spawn_control_inbox(src_rx: Receiver<WorkerCmd>) -> io::Result<ControlInbox> {
    let (fwd_tx, fwd_rx) = mpsc::channel();
    let wake = Arc::new(Mutex::new(None::<Waker>));
    let wake_clone = Arc::clone(&wake);
    thread::Builder::new()
        .name("nmp-broker-control".to_string())
        .spawn(move || forward_commands(src_rx, fwd_tx, wake_clone))?;
    Ok(ControlInbox { rx: fwd_rx, wake })
}

fn forward_commands(
    src: Receiver<WorkerCmd>,
    fwd: Sender<WorkerCmd>,
    wake: Arc<Mutex<Option<Waker>>>,
) {
    // Block on `recv()` (no polling). When a command arrives, forward it and
    // fire the mio waker so the worker wakes from its `poll.poll(None)`.
    while let Ok(cmd) = src.recv() {
        if fwd.send(cmd).is_err() {
            return;
        }
        if let Ok(slot) = wake.lock() {
            if let Some(waker) = slot.as_ref() {
                let _ = waker.wake();
            }
        }
    }
}

impl ControlInbox {
    fn install_waker(&self, waker: Waker) -> ControlWakeGuard {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = Some(waker);
        }
        ControlWakeGuard {
            wake: Arc::clone(&self.wake),
        }
    }

    /// Drain queued commands into `pending` / `subscriptions`. Subscription
    /// frames are immediately written to the socket AND persisted for
    /// reconnect replay.
    fn drain(
        &self,
        pending: &mut VecDeque<String>,
        subscriptions: &mut Vec<String>,
        socket: &mut RelaySocket,
    ) -> ControlDrain {
        loop {
            match self.rx.try_recv() {
                Ok(WorkerCmd::Send(frame)) => pending.push_back(frame),
                Ok(WorkerCmd::Subscribe(frame)) => {
                    // Persist first so a write-failure path still has the
                    // frame for replay on the next connect.
                    subscriptions.push(frame.clone());
                    if socket.send(Message::Text(frame)).is_err() {
                        return ControlDrain::WriteFailed;
                    }
                }
                Ok(WorkerCmd::Shutdown) => return ControlDrain::Shutdown,
                Err(TryRecvError::Empty) => return ControlDrain::Continue,
                Err(TryRecvError::Disconnected) => return ControlDrain::Disconnected,
            }
        }
    }

    /// Block (with timeout) waiting for a control command. Used by the
    /// pre-connect / inter-reconnect backoff so we honour Shutdown
    /// immediately and queue Send/Subscribe frames that arrive during the
    /// wait. `recv_timeout` is a blocking primitive — not polling (D8).
    fn recv_timeout(&self, timeout: Duration) -> Result<WorkerCmd, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}

struct ControlWakeGuard {
    wake: Arc<Mutex<Option<Waker>>>,
}

impl Drop for ControlWakeGuard {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = None;
        }
    }
}

/// Non-blocking peek: drain every queued command. Send/Subscribe frames
/// are buffered for the next connected session; a Shutdown short-circuits
/// the supervisor. Returns `true` iff Shutdown was observed (caller must
/// exit). Used between reconnect attempts so a cancel that arrived during
/// the prior backoff aborts before we burn another TCP/TLS handshake.
///
/// `try_recv` is permitted here as the readiness-helper carve-out: this
/// drains a queue, it does not poll for events. See AGENTS.md "No polling
/// — ever" and the doctrine guard at the bottom of this file.
fn drain_for_shutdown(
    control: &ControlInbox,
    pending: &mut VecDeque<String>,
    subscriptions: &mut Vec<String>,
) -> bool {
    loop {
        match control.rx.try_recv() {
            Ok(WorkerCmd::Send(frame)) => pending.push_back(frame),
            Ok(WorkerCmd::Subscribe(frame)) => subscriptions.push(frame),
            Ok(WorkerCmd::Shutdown) => return true,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => return true,
        }
    }
}

/// Block until either the backoff deadline elapses or a control command
/// arrives. Send/Subscribe frames are queued and replayed on the next
/// connect; Shutdown aborts. Returns `true` to continue the supervisor
/// loop, `false` to exit cleanly.
fn wait_before_reconnect(
    control: &ControlInbox,
    pending: &mut VecDeque<String>,
    subscriptions: &mut Vec<String>,
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
            Ok(WorkerCmd::Send(frame)) => pending.push_back(frame),
            Ok(WorkerCmd::Subscribe(frame)) => subscriptions.push(frame),
            Ok(WorkerCmd::Shutdown) => return false,
            Err(RecvTimeoutError::Timeout) => return true,
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

// ─── Socket I/O ──────────────────────────────────────────────────────────────

enum FlushResult {
    Flushed,
    Blocked,
    Reconnect,
}

fn flush_pending(socket: &mut RelaySocket, pending: &mut VecDeque<String>) -> FlushResult {
    while let Some(text) = pending.pop_front() {
        match socket.write(Message::Text(text.clone())) {
            Ok(()) => {}
            Err(err) if is_nonblocking_io(&err) => {
                pending.push_front(text);
                return FlushResult::Blocked;
            }
            Err(_) => {
                pending.push_front(text);
                return FlushResult::Reconnect;
            }
        }
    }
    match socket.flush() {
        Ok(()) => FlushResult::Flushed,
        Err(err) if is_nonblocking_io(&err) => FlushResult::Blocked,
        Err(_) => FlushResult::Reconnect,
    }
}

enum ReadResult {
    Continue,
    Closed,
    Permanent,
}

fn drain_reads(socket: &mut RelaySocket, on_event: &EventCallback) -> ReadResult {
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Some(value) = parse_event_frame(&text) {
                    on_event(value);
                }
            }
            Ok(Message::Binary(_)) => {
                // NIP-01 is text-only; ignore.
            }
            Ok(Message::Ping(payload)) => {
                // Best-effort pong; a failed write surfaces as Reconnect on
                // the next flush. We deliberately don't try to write here
                // directly because the socket is non-blocking and the write
                // half may be back-pressured.
                if socket.send(Message::Pong(payload)).is_err() {
                    return ReadResult::Closed;
                }
            }
            Ok(Message::Pong(_) | Message::Frame(_)) => {}
            Ok(Message::Close(_)) => return ReadResult::Closed,
            Err(err) if is_nonblocking_io(&err) => return ReadResult::Continue,
            Err(err) => {
                if is_permanent_error(&err.to_string()) {
                    return ReadResult::Permanent;
                }
                return ReadResult::Closed;
            }
        }
    }
}

fn is_nonblocking_io(err: &tungstenite::Error) -> bool {
    matches!(
        err,
        tungstenite::Error::Io(io_err)
            if matches!(
                io_err.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            )
    )
}

fn open_socket(url: &str) -> Result<RelaySocket, String> {
    install_rustls_provider();
    let (socket, _resp) = tungstenite::connect(url).map_err(|e| e.to_string())?;
    Ok(socket)
}

fn socket_tcp(socket: &mut RelaySocket) -> io::Result<&mut TcpStream> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => Ok(stream),
        MaybeTlsStream::Rustls(stream) => Ok(stream.get_mut()),
        #[allow(unreachable_patterns)]
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported tungstenite stream variant",
        )),
    }
}

fn set_nonblocking(socket: &mut RelaySocket, nonblocking: bool) -> io::Result<()> {
    socket_tcp(socket)?.set_nonblocking(nonblocking)
}

fn register_socket(
    poll: &Poll,
    socket: &mut RelaySocket,
    wants_write: bool,
    already_registered: bool,
) -> io::Result<()> {
    let fd = socket_tcp(socket)?.as_raw_fd();
    let interest = if wants_write {
        Interest::READABLE.add(Interest::WRITABLE)
    } else {
        Interest::READABLE
    };
    let mut source = SourceFd(&fd);
    if already_registered {
        poll.registry().reregister(&mut source, SOCKET, interest)
    } else {
        poll.registry().register(&mut source, SOCKET, interest)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    #[test]
    fn parse_event_frame_rejects_non_array_json() {
        // A bare object or scalar must not panic — D6.
        assert!(parse_event_frame(r#"{"id":"abc"}"#).is_none());
        assert!(parse_event_frame(r#"42"#).is_none());
        assert!(parse_event_frame(r#""just-a-string""#).is_none());
    }

    #[test]
    fn relay_error_display_strings_are_descriptive() {
        // Display strings flow into `BunkerHandshakeProgress` failure text;
        // they must carry the cause without panicking.
        assert_eq!(
            RelayError::Connect("tls handshake".to_string()).to_string(),
            "connect failed: tls handshake"
        );
        assert_eq!(
            RelayError::Write("broken pipe".to_string()).to_string(),
            "write failed: broken pipe"
        );
        assert_eq!(
            RelayError::Disconnected.to_string(),
            "relay client disconnected"
        );
    }

    #[test]
    fn default_subscribe_forwards_to_send() {
        // Stub impls that don't override `subscribe` should still receive
        // the frame via `send`, so they keep working without changes.
        struct CountingStub {
            send_count: AtomicUsize,
        }
        impl RelayClient for CountingStub {
            fn send(&self, _frame: String) -> Result<(), RelayError> {
                self.send_count.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            fn shutdown(&self) {}
        }
        let stub = CountingStub {
            send_count: AtomicUsize::new(0),
        };
        stub.subscribe("[\"REQ\",\"x\",{}]".to_string()).unwrap();
        assert_eq!(stub.send_count.load(Ordering::Relaxed), 1);
    }

    // ─── No-polling doctrine guard ─────────────────────────────────────────
    //
    // Mirror `nmp-core::relay_worker::no_polling_tests`: assert that the
    // PRODUCTION source of this file does not regress to the banned patterns.
    // We split the file at the `#[cfg(test)]` marker so the literal banned
    // tokens that appear in this very test (as the `forbidden` array) don't
    // false-positive — only code reachable in a release build is scanned.
    // Treat this as the canonical test for V-13.
    #[test]
    fn relay_client_uses_readiness_not_fixed_read_timeouts() {
        let full = include_str!("relay_client.rs");
        let production = full
            .split("#[cfg(test)]")
            .next()
            .expect("source has a production half");
        for forbidden in ["set_read_timeout", "Duration::from_millis(100)"] {
            assert!(
                !production
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .filter(|l| !l.trim_start().starts_with("//!"))
                    .any(|l| l.contains(forbidden)),
                "relay client regressed to polling pattern: {forbidden}"
            );
        }
        assert!(
            production.contains("Poll::new()") && production.contains("Waker::new"),
            "relay client should block on socket readiness and control-channel wakeups"
        );
    }
}
