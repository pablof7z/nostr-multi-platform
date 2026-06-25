//! Thin adapter from `nmp_network::Pool` to the broker's `RelayClient` trait
//! (step 8 phase D — V-13 Stage 2 dedupe).
//!
//! Before phase D the broker carried its own ~700-line mio/tungstenite
//! readiness loop — a near-line-for-line duplicate of `nmp-core::relay_worker`.
//! Phase B (PR #470) shipped `nmp_network::Pool`; phase D (this rewrite)
//! deletes the duplicate and reuses `Pool`. `crate-boundaries.md`:
//! **"One readiness-driven WebSocket implementation in the workspace,
//! period."**
//!
//! ## Decision: one [`nmp_network::Pool`] per session
//!
//! The broker constructs a fresh `Pool` per active session rather than
//! sharing the kernel's. Bunker relays are not the user's app relays; the
//! bunker URI dictates which relays to dial. Lifecycle isolation: `cancel()`
//! tears down the session's pool wholesale (`Pool::shutdown`). Cost: one
//! extra translator thread per session — sessions are typically singleton,
//! so this is negligible.
//!
//! ## V-14 invariants preserved
//!
//! - **Mid-session reconnect** is provided by `nmp_network::relay_worker`
//!   (jittered exponential backoff 3 s → 300 s; byte-for-byte the prior
//!   in-broker policy).
//! - **Subscription replay** is what the broker still drives: the Pool is
//!   a wire primitive, not a NIP-01 stateful session, so it does not
//!   auto-replay client frames. [`PoolRelayClient::subscribe`] stores the
//!   REQ frame and the dispatcher re-sends every stored subscription on
//!   each fresh `PoolEvent::Opened` — so the inbound REQ survives a flap.
//!
//! The [`RelayClient`] trait surface is unchanged; only the production
//! impl is replaced. [`TungsteniteRelayClient`] is kept as a type alias
//! to [`PoolRelayClient`] for legacy spelling.

mod dispatch;

#[cfg(test)]
pub(crate) use dispatch::{closed_reason_to_state, parse_event_frame, transport_error_to_state};
use dispatch::{run_dispatcher, wait_for_first_open};

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::Value;

use nmp_network::pool::{Pool, PoolConfig, PoolEvent, RelayHandle, WireFrame};

/// How long [`PoolRelayClient::connect`] waits for the worker's first
/// `PoolEvent::Opened` (or `Failed`) before returning. The Pool dials
/// asynchronously, but the broker's `connect_session` cycles between URLs
/// on failure and needs sync feedback to know when to pivot. 10 s covers
/// a TLS handshake against any reachable relay.
const CONNECT_BUDGET: Duration = Duration::from_secs(10);

/// Subscription id used for the signer-broker inbound REQ.
pub(crate) const BUNKER_SUB_ID: &str = "nmp-bunker";

/// Signature of the inbound event callback. Receives the raw event JSON
/// `Value` (the third element of `["EVENT", <sub_id>, <event_json>]`).
/// MUST be cheap (called on the dispatcher thread); offload work if needed.
pub type EventCallback = Arc<dyn Fn(Value) + Send + Sync>;

/// Signature of the connection-state callback. Called on the dispatcher
/// thread when the relay-layer connection transitions between
/// `"connected"` / `"reconnecting"` / `"failed"`. V-14 step b: the broker
/// adapter translates these into a `BrokerEvent::ConnectionStateChanged`
/// and routes it through `ActorCommand::BunkerConnectionStateChanged` so
/// the snapshot projection is updated on the actor thread (D4).
///
/// `state` is one of `"connected"`, `"reconnecting"`, or `"failed"`.
/// `reason` is `Some(msg)` for `"reconnecting"` and `"failed"`.
pub type ConnectionStateCallback = Arc<dyn Fn(&str, Option<&str>) + Send + Sync>;

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

/// Trait the broker programs against. Production: [`PoolRelayClient`].
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

    /// Cancel the worker, close the socket, AND block until the client's own
    /// background thread (if any) is joined. Idempotent. Convenience for
    /// callers that are NOT on a latency-sensitive path (e.g. `Drop`).
    ///
    /// On the actor / capability call path use [`Self::signal_shutdown`]
    /// instead — `shutdown` may block on a join, which freezes the actor when a
    /// relay worker is stuck mid-connect.
    fn shutdown(&self) {
        // Default: signal, then join the surrendered handle inline. Stubs that
        // have no background thread return `None` and this is a pure signal.
        if let Some(handle) = self.signal_shutdown() {
            let _ = handle.join();
        }
    }

    /// **Signal-only shutdown.** Cancel the worker and close the socket WITHOUT
    /// blocking, and surrender the client's own background-thread join handle
    /// (if any) to the caller so a detached reaper can join it off the
    /// latency-sensitive path. Idempotent: a second call returns `None`.
    ///
    /// This is the D4 contract for `BunkerBroker::cancel()`: the relay teardown
    /// signal (drop senders / send `WorkerCmd::Shutdown`) happens here on the
    /// caller path (it is non-blocking), and the join the signal eventually
    /// unblocks is performed by the reaper, never the actor.
    ///
    /// Default impl (test stubs with no background thread): signal via
    /// [`Self::shutdown`]-equivalent is a no-op and we return `None`.
    fn signal_shutdown(&self) -> Option<JoinHandle<()>> {
        None
    }
}

/// Pool-backed relay client. Owns one [`Pool`] (with one [`RelayHandle`])
/// for the lifetime of the active session. The Pool's translator delivers
/// inbound `PoolEvent`s on a channel; this client's dispatcher thread
/// parses kind-24133 frames out of `["EVENT", sub_id, event_json]`
/// envelopes, fires the user-supplied [`EventCallback`], and re-replays
/// installed subscriptions on every reconnect (V-14).
///
/// V-14 step b: accepts an optional [`ConnectionStateCallback`] that is
/// invoked on `Opened` (→ `"connected"`), `Closed` (→ `"reconnecting"`
/// unless `ClosedReason::Permanent`/`Shutdown` → `"failed"`), and
/// `Failed` (→ `"reconnecting"` for transient / `"failed"` for permanent)
/// so the broker can emit a `BrokerEvent::ConnectionStateChanged` without
/// polling.
pub struct PoolRelayClient {
    pool: Pool,
    handle: RelayHandle,
    /// Subscriptions installed via [`Self::subscribe`]. Replayed after
    /// every `PoolEvent::Opened` so the inbound REQ survives a transient
    /// drop (V-14). Locked only for short windows during install / replay.
    subscriptions: Arc<Mutex<Vec<String>>>,
    /// Joined on [`Self::shutdown`]. The dispatcher exits when the Pool's
    /// translator drops its event sender (which `Pool::shutdown` triggers
    /// indirectly via worker shutdown), so we don't need a separate
    /// shutdown signal — D8 compliant blocking `recv`.
    dispatcher: Mutex<Option<JoinHandle<()>>>,
}

impl PoolRelayClient {
    /// Construct a client that dials `url` via a fresh [`Pool`] and invokes
    /// `on_event` for every inbound NIP-01 EVENT frame. Blocks up to
    /// [`CONNECT_BUDGET`] for the first `PoolEvent::Opened` (success) or
    /// `PoolEvent::Failed` / timeout (return Err, so the broker's
    /// `connect_session` cycle pivots to the next URL). Once `Ok` returns,
    /// mid-session reconnect is fully transparent: the worker handles
    /// backoff and the dispatcher replays subscriptions on each fresh
    /// `Opened` (V-14).
    ///
    /// V-14 step b: `on_connection_state` is an optional callback invoked
    /// on relay lifecycle events (`Opened` → `"connected"`, transient
    /// `Closed`/`Failed` → `"reconnecting"`, permanent `Closed`/`Failed`
    /// → `"failed"`). Pass `None` for callers that don't need it.
    pub fn connect(
        url: &str,
        on_event: EventCallback,
        on_connection_state: Option<ConnectionStateCallback>,
    ) -> Result<Self, RelayError> {
        // Per-session pool: the broker's relays are not the user's relays,
        // so we don't share the kernel's pool. See module docs for the
        // full rationale.
        let (pool_events_tx, pool_events_rx) = mpsc::channel::<PoolEvent>();
        let pool = Pool::new(PoolConfig::default(), pool_events_tx);
        let handle = pool.ensure_open(&url.to_string());

        // Block (with budget) for the first Opened / hard Failed. Stray
        // events that arrive during the wait are forwarded to the
        // dispatcher's input via the same `pool_events_rx` (consumed
        // below). Non-Opened/non-permanent events during the wait are
        // buffered and replayed after the dispatcher starts.
        let mut buffered: Vec<PoolEvent> = Vec::new();
        let connect_result = wait_for_first_open(&pool_events_rx, &mut buffered, CONNECT_BUDGET);
        if let Err(e) = connect_result {
            // Tear down the Pool so the worker stops dialing this URL.
            pool.shutdown();
            return Err(e);
        }

        let subscriptions = Arc::new(Mutex::new(Vec::<String>::new()));
        let pool_for_dispatch = pool.clone();
        let subs_for_dispatch = Arc::clone(&subscriptions);
        let dispatcher = thread::Builder::new()
            .name("nmp-broker-pool-dispatch".to_string())
            .spawn(move || {
                run_dispatcher(
                    pool_events_rx,
                    pool_for_dispatch,
                    handle,
                    subs_for_dispatch,
                    on_event,
                    on_connection_state,
                    buffered,
                );
            })
            .map_err(|e| RelayError::Connect(format!("spawn dispatcher: {e}")))?;

        Ok(Self {
            pool,
            handle,
            subscriptions,
            dispatcher: Mutex::new(Some(dispatcher)),
        })
    }
}

impl RelayClient for PoolRelayClient {
    fn send(&self, frame: String) -> Result<(), RelayError> {
        if self.pool.send(self.handle, WireFrame::Text(frame)) {
            Ok(())
        } else {
            // Pool::send returns false only when the handle is stale OR
            // the inner lock is poisoned OR the worker channel is gone.
            // Surface as Disconnected so the caller fails fast instead of
            // dropping the frame silently (matches the prior client's
            // contract: a dropped sign request must never look published).
            Err(RelayError::Disconnected)
        }
    }

    fn subscribe(&self, frame: String) -> Result<(), RelayError> {
        // Persist BEFORE sending so a write that fails (and triggers a
        // worker reconnect) still has the frame queued for the
        // dispatcher's next `Opened`-driven replay.
        if let Ok(mut subs) = self.subscriptions.lock() {
            subs.push(frame.clone());
        }
        // Pool::send may return false on the very first call if the
        // worker is still mid-connect — but the worker's pending queue
        // accepts the frame and flushes it on open, so we still want to
        // report success here. Treat false as "queued / will retry on
        // open" rather than an error. (Failure here would prevent the
        // handshake REQ from ever installing.)
        let _ = self.pool.send(self.handle, WireFrame::Text(frame));
        Ok(())
    }

    fn signal_shutdown(&self) -> Option<JoinHandle<()>> {
        // Pool::shutdown is itself non-blocking: it signals every worker
        // (`RelayCommand::Shutdown` + shutdown flag) AND swaps the public
        // events sender for a dead channel. It does NOT join the worker /
        // translator threads. The dead-channel swap is load-bearing: we still
        // own `self.pool` while the dispatcher is joined (by the caller's
        // reaper), so the original `PoolInner.events` sender would otherwise
        // stay alive (held by the inner `Arc<Mutex<_>>`) and the dispatcher's
        // `pool_events_rx.recv()` would block indefinitely. With the swap, the
        // original sender drops at shutdown time and `recv()` resolves once the
        // translator finishes draining — no parallel shutdown signal, no
        // polling.
        //
        // D4: we surrender the dispatcher join handle to the caller rather than
        // joining it here. `cancel()` runs on the actor path; the dispatcher's
        // `recv()` only resolves after the relay worker exits (which can take
        // until a stuck connect bounds out), so joining it here would freeze
        // the actor. The reaper joins it off-path instead.
        self.pool.shutdown();
        self.dispatcher.lock().ok().and_then(|mut g| g.take())
    }
}

impl Drop for PoolRelayClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl std::fmt::Debug for PoolRelayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PoolRelayClient").finish_non_exhaustive()
    }
}

/// Legacy spelling — kept so call-sites that explicitly named the prior
/// tungstenite-backed implementation continue to compile. New code should
/// use [`PoolRelayClient`] directly.
pub type TungsteniteRelayClient = PoolRelayClient;

#[cfg(test)]
mod tests;
