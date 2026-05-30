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

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::Value;

use nmp_network::pool::{Pool, PoolConfig, PoolEvent, RelayFrame, RelayHandle, WireFrame};

/// How long [`PoolRelayClient::connect`] waits for the worker's first
/// `PoolEvent::Opened` (or `Failed`) before returning. The Pool dials
/// asynchronously, but the broker's `connect_session` cycles between URLs
/// on failure and needs sync feedback to know when to pivot. 10 s covers
/// a TLS handshake against any reachable relay.
const CONNECT_BUDGET: Duration = Duration::from_secs(10);

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

    /// Cancel the worker, close the socket. Idempotent.
    fn shutdown(&self);
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

/// Block on `events` until `Opened` (Ok), first `Failed` (Err — broker
/// pivots to next URL), or budget elapses (Err). Non-terminal events seen
/// during the wait are appended to `buffered` so the dispatcher can
/// replay them after it spins up.
///
/// We bail on the first `Failed` because the broker's `connect_session`
/// loop is the authority on URL ordering; if we let the Pool's internal
/// jittered retry kick in (3 s → 6 s → …) the broker couldn't pivot to
/// its second relay until a full backoff cycle elapsed.
fn wait_for_first_open(
    events: &Receiver<PoolEvent>,
    buffered: &mut Vec<PoolEvent>,
    budget: Duration,
) -> Result<(), RelayError> {
    let deadline = std::time::Instant::now() + budget;
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            return Err(RelayError::Connect(format!(
                "no relay open within {budget:?}"
            )));
        }
        match events.recv_timeout(remaining) {
            Ok(ev) => match ev {
                PoolEvent::Opened { .. } => {
                    buffered.push(ev);
                    return Ok(());
                }
                PoolEvent::Failed { ref error, .. } => {
                    return Err(RelayError::Connect(error.message.clone()));
                }
                other => buffered.push(other),
            },
            Err(RecvTimeoutError::Timeout) => {
                return Err(RelayError::Connect(format!(
                    "no relay open within {budget:?}"
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(RelayError::Connect(
                    "pool translator disconnected before open".to_string(),
                ));
            }
        }
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

    fn shutdown(&self) {
        // Pool::shutdown signals every worker AND swaps the public
        // events sender for a dead channel. The dead-channel swap is
        // load-bearing: we still own `self.pool` while joining the
        // dispatcher below, so the original `PoolInner.events` sender
        // would otherwise stay alive (held by the inner `Arc<Mutex<_>>`)
        // and the dispatcher's `pool_events_rx.recv()` would block
        // indefinitely. With the swap, the original sender drops at
        // shutdown time and `recv()` resolves naturally — no parallel
        // shutdown signal, no polling.
        self.pool.shutdown();
        if let Ok(mut guard) = self.dispatcher.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
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

// ─── Dispatcher: PoolEvent → EventCallback ──────────────────────────────────

/// Pool-event dispatcher. Blocks on `pool_events_rx` (D8: no polling) until
/// the Pool's translator drops its sender (triggered indirectly by
/// `Pool::shutdown`). On `Opened` replays every stored subscription (V-14)
/// and fires `on_connection_state("connected", None)`. On `Closed`/`Failed`
/// fires `on_connection_state("reconnecting"|"failed", reason)` (V-14 step b).
/// On `Frame(Text)` parses the NIP-01 EVENT envelope and fires `on_event`.
fn run_dispatcher(
    pool_events_rx: Receiver<PoolEvent>,
    pool: Pool,
    handle: RelayHandle,
    subscriptions: Arc<Mutex<Vec<String>>>,
    on_event: EventCallback,
    on_connection_state: Option<ConnectionStateCallback>,
    buffered: Vec<PoolEvent>,
) {
    // Replay events that arrived during the connect-wait first — the
    // Opened we waited for is in here too, so its subscription-replay
    // pass fires before we enter the recv loop.
    for ev in buffered {
        handle_pool_event(ev, &pool, handle, &subscriptions, &on_event, &on_connection_state);
    }
    while let Ok(ev) = pool_events_rx.recv() {
        handle_pool_event(ev, &pool, handle, &subscriptions, &on_event, &on_connection_state);
    }
}

fn handle_pool_event(
    ev: PoolEvent,
    pool: &Pool,
    handle: RelayHandle,
    subscriptions: &Arc<Mutex<Vec<String>>>,
    on_event: &EventCallback,
    on_connection_state: &Option<ConnectionStateCallback>,
) {
    match ev {
        PoolEvent::Opened { .. } => {
            // V-14: replay every installed REQ on each fresh open so the
            // inbound subscription survives a reconnect.
            let frames: Vec<String> = subscriptions
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();
            for frame in frames {
                let _ = pool.send(handle, WireFrame::Text(frame));
            }
            // V-14 step b: relay connected (or reconnected after a flap).
            if let Some(cb) = on_connection_state {
                cb("connected", None);
            }
        }
        PoolEvent::Frame {
            frame: RelayFrame::Text(text),
            ..
        } => {
            if let Some(value) = parse_event_frame(&text) {
                on_event(value);
            }
        }
        // Binary/Ping/Pong/Close — NIP-01 is text-only; keepalive is
        // handled inside the Pool's translator.
        PoolEvent::Frame { .. } => {}
        // V-14 step b: transient mid-session close — the Pool will auto-
        // reconnect (jittered backoff). Report `"reconnecting"` so the host
        // can show a reconnecting indicator.
        PoolEvent::Closed { reason, .. } => {
            use nmp_network::pool::ClosedReason;
            let is_permanent = matches!(reason, ClosedReason::Permanent | ClosedReason::Shutdown);
            let state = if is_permanent { "failed" } else { "reconnecting" };
            if let Some(cb) = on_connection_state {
                cb(state, None);
            }
        }
        // V-14 step b: Failed — transient errors trigger Pool reconnect;
        // permanent errors (HTTP 401/403) stop reconnect and brick the session.
        PoolEvent::Failed { error, .. } => {
            let state = if error.permanent { "failed" } else { "reconnecting" };
            if let Some(cb) = on_connection_state {
                cb(state, Some(error.message.as_str()));
            }
        }
        // Health snapshots carry no state change — they aggregate counters
        // already reflected in Opened/Closed/Failed.
        PoolEvent::Health { .. } => {}
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

    // ─── No-polling doctrine guard (V-13 Stage 2) ──────────────────────────
    //
    // The V-13 Stage 2 dedupe replaced the in-broker mio/tungstenite loop
    // with a thin wrapper over `nmp_network::Pool`. Assert that the
    // banned patterns (set_read_timeout, fixed-ms try_recv polling) are
    // gone from THIS file. The underlying readiness loop now lives in
    // `nmp-network`'s `relay_worker` and is guarded by its own
    // no-polling test (`relay_worker::no_polling_tests`).
    #[test]
    fn relay_client_uses_pool_not_polling() {
        let full = include_str!("relay_client.rs");
        let production = full
            .split("#[cfg(test)]")
            .next()
            .expect("source has a production half");
        for forbidden in [
            "set_read_timeout",
            "Duration::from_millis(100)",
            "tungstenite::WebSocket",
            "MaybeTlsStream",
            "mio::Poll",
        ] {
            assert!(
                !production
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .filter(|l| !l.trim_start().starts_with("//!"))
                    .any(|l| l.contains(forbidden)),
                "relay client regressed to in-broker socket pattern: {forbidden}"
            );
        }
        assert!(
            production.contains("nmp_network::pool::Pool")
                || production.contains("use nmp_network::pool"),
            "relay client must route through nmp_network::Pool (V-13 Stage 2)"
        );
    }
}
