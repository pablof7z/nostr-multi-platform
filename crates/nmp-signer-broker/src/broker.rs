//! `BunkerBroker` — top-level coordinator.
//!
//! Owns:
//! - A host-installed event callback for reporting progress and completed
//!   signers without naming app or actor types.
//! - At most one `ActiveSession` (relay client + transport + handshake
//!   thread). MVP supports a single concurrent bunker; a follow-up can key a
//!   `HashMap<bunker_url, ActiveSession>`.
//!
//! Lifecycle:
//! - `start_handshake(uri)` validates the URI, opens a relay client,
//!   subscribes to inbound responses, spawns a worker thread that drives the
//!   handshake state machine, and reports progress to the host callback.
//! - `cancel()` flips the active session's `AtomicBool` cancel flag, tears
//!   down the relay client, and **detaches** the session's worker threads onto
//!   a background reaper (it never joins them on the caller's path). Idempotent.
//!
//! Threading: every method here is non-blocking from the caller's POV — and
//! `cancel()` in particular must never block, because it runs on the actor /
//! capability call path. A blocking join there froze the actor for up to the
//! relay connect timeout — the bug ADR-0050's signer-session port set out to
//! kill ("bunker cancel is detach — signal and drop — not join"). The actual
//! relay I/O and handshake protocol runs on a dedicated worker thread per call;
//! the broker keeps the join handles only so a detached reaper can reclaim them
//! after the signal makes them self-exit. No thread is leaked: the reaper owns
//! and joins every handle off the caller's path.

mod nostrconnect;
mod restore;
#[cfg(test)]
mod tests;

use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use nmp_signers::{parse_bunker_uri, Nip46Signer, Nip46SignerHandle};
use nostr::{Keys, PublicKey};
use serde_json::Value;

use crate::events::{BrokerEvent, BrokerEventHandler};
use crate::handshake::{build_req_frame, run_handshake, HandshakeOutcome};
use crate::relay_client::{EventCallback, RelayClient, TungsteniteRelayClient};
use crate::transport::BrokerTransport;

/// Subscription id used for the inbound REQ. One per session is enough.
const BUNKER_SUB_ID: &str = "nmp-bunker";

/// Upper bound the cancel reaper waits on any single thread join before
/// abandoning it. The relay worker's connect path has steps that are not
/// covered by `nmp-network`'s in-connect `TCP_CONNECT_TIMEOUT` (DNS resolution
/// and the TLS/HTTP-upgrade read on a connected-but-silent socket); a worker
/// wedged there only observes `WorkerCmd::Shutdown` once the OS-level timeout
/// fires. This budget caps how long the (detached) reaper lives — the worker
/// is parked in a syscall, not spinning, so abandoning the join leaks nothing
/// unboundedly. 30 s comfortably exceeds the bounded `TCP_CONNECT_TIMEOUT`
/// (10 s) plus translator drain, so a normally-terminating worker is always
/// joined within budget; only a genuinely OS-wedged dial is abandoned.
const REAP_JOIN_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// Top-level broker. Host composition owns the app/actor adapter and passes
/// its event callback here.
pub struct BunkerBroker {
    events: Arc<BrokerEventHandler>,
    active: Mutex<Option<ActiveSession>>,
    /// ADR-0050 §D3b completion sink, installed once by the host adapter via
    /// [`BunkerBroker::set_completion_sink`] and bound onto each session's
    /// transport in `install_completed_signer`. `None` until the host installs
    /// it (the host always does at broker init). The broker never names
    /// `ActorCommand` (D0) — it sees only the opaque `Fn(String)`.
    completion_sink: Mutex<Option<crate::CompletionSink>>,
}

struct ActiveSession {
    relay: Arc<dyn RelayClient>,
    cancel: Arc<AtomicBool>,
    handshake_thread: Option<JoinHandle<()>>,
    /// Inbound-dispatcher thread spawned by `install_completed_signer` (routes
    /// steady-state kind:24133 replies to the transport). Stored so `cancel()`
    /// can hand it to the background reaper after signalling shutdown — without
    /// this handle the dispatcher thread leaked on every session teardown,
    /// accumulating one stuck thread per rapid reconnect. The reaper, not the
    /// caller, joins it. `None` until the handshake completes and the signer is
    /// installed.
    dispatcher_thread: Option<JoinHandle<()>>,
    /// Strong ref to the transport so the relay-event callback can reach it.
    /// Kept here so we can drop it on `cancel`.
    transport: Arc<BrokerTransport>,
    /// Strong ref to the signer once handshake completes. Dropped on
    /// `cancel` or when the host drops the account.
    signer: Mutex<Option<Arc<Nip46Signer>>>,
}

impl BunkerBroker {
    /// Construct a new broker with the host event callback.
    #[must_use]
    pub fn new(events: Arc<BrokerEventHandler>) -> Arc<Self> {
        Arc::new(Self {
            events,
            active: Mutex::new(None),
            completion_sink: Mutex::new(None),
        })
    }

    /// Install the ADR-0050 §D3b completion sink (host → `DeliverSignerResponse`
    /// command). Bound onto each session's transport when the signer is
    /// installed, so every steady-state kind:24133 reply is routed to the actor
    /// inbox instead of being resolved on the dispatcher thread. Call once at
    /// broker init; subsequent sessions reuse it.
    pub fn set_completion_sink(&self, sink: crate::CompletionSink) {
        if let Ok(mut slot) = self.completion_sink.lock() {
            *slot = Some(sink);
        }
    }

    /// Begin handshake for a `bunker://` URI. Returns immediately; the
    /// actual work runs on a worker thread. Cancels any prior in-flight
    /// session first (MVP — single-session).
    pub fn start_handshake(self: &Arc<Self>, uri: String) {
        // Cancel any prior session so a re-submit replaces cleanly.
        self.cancel();

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        let me = Arc::clone(self);

        // Spawn under the lock: the worker's first contention point is its
        // own `self.active.lock()` inside `install_session`, which will block
        // until this scope releases the guard. That guarantees the placeholder
        // is staged before the worker can mutate it — closing the race where a
        // fast worker could reach `install_session` before staging and have
        // its real relay/transport silently dropped (since `install_session`
        // is a mutate-if-Some no-op).
        if let Ok(mut guard) = self.active.lock() {
            let thread =
                std::thread::spawn(move || me.run_handshake_thread(uri, cancel_for_thread));
            *guard = Some(ActiveSession {
                // Placeholder relay reference until the worker swaps it in.
                // We use an `Arc<NoopRelay>` so the field type stays simple.
                relay: Arc::new(NoopRelay) as Arc<dyn RelayClient>,
                cancel,
                handshake_thread: Some(thread),
                dispatcher_thread: None,
                transport: BrokerTransport::new(
                    Arc::new(NoopRelay) as Arc<dyn RelayClient>,
                    Keys::generate(),
                    Keys::generate().public_key(),
                ),
                signer: Mutex::new(None),
            });
        }
    }

    /// Cancel the active session if any. Idempotent.
    ///
    /// **Signal-only / detached.** This runs on the actor / capability call
    /// path, so it MUST NOT block. It signals shutdown (set the cancel flag,
    /// drain pending signs, shut down the relay client) and then hands the
    /// session's worker handles to a detached background **reaper** thread that
    /// joins them off this path. The caller returns immediately, even while a
    /// dispatcher thread is still winding down or a relay worker is stuck mid
    /// connect — eliminating the up-to-connect-timeout actor freeze a join on
    /// this path used to cause (ADR-0050: "bunker cancel is detach, not join").
    /// No thread leaks: the reaper owns and joins every handle.
    pub fn cancel(&self) {
        let session = if let Ok(mut guard) = self.active.lock() {
            guard.take()
        } else {
            None
        };
        if let Some(session) = session {
            // Drain any in-flight sign requests so callers fail fast instead
            // of waiting out PENDING_SIGN_TIMEOUT (5s). The signer's pending
            // map still holds the response senders for requests already
            // submitted to the broker; without this they would be orphaned.
            if let Ok(slot) = session.signer.lock() {
                if let Some(signer) = slot.as_ref() {
                    signer.drain_pending_with_error("bunker session cancelled");
                }
            }
            // Release store pairs with the Acquire loads in the handshake /
            // nostrconnect loops so the cancel is guaranteed visible across
            // threads (and ARM cores — iOS/Android — where Relaxed grants no
            // happens-before). The reader observing `true` is thereby ordered
            // after this store.
            session
                .cancel
                .store(true, std::sync::atomic::Ordering::Release);
            // SIGNAL-ONLY relay teardown. `signal_shutdown()` drops the event
            // callback's `inbound_tx` clone (closing the broker dispatcher's
            // channel), tells the relay worker to exit (`WorkerCmd::Shutdown`),
            // and *surrenders* the relay client's own dispatcher join handle
            // WITHOUT blocking on it. The relay client's `recv()` only resolves
            // after the relay worker exits (which can take until a stuck connect
            // bounds out), so joining it here would re-introduce the actor
            // freeze D4 removes. The reaper joins it instead.
            let relay_dispatcher = session.relay.signal_shutdown();

            // Detach EVERY join onto a background reaper — the actor call path
            // performs zero `join()`. The threads self-exit on the signals we
            // just raised:
            //   - The handshake thread observes `cancel` via the
            //     `await_response` recv_timeout loop in handshake.rs (wakes
            //     within ~200ms), or the relay worker it drives exits on the
            //     `WorkerCmd::Shutdown` from `signal_shutdown()`.
            //   - The broker inbound dispatcher's blocking `inbound_rx.recv()`
            //     returns `Err` once every `inbound_tx` sender is dropped: the
            //     relay's event-callback clone (dropped when the relay client's
            //     own dispatcher exits) and the handshake thread's original
            //     sender (dropped when that thread returns).
            //   - The relay client's dispatcher exits when the Pool's translator
            //     drops its event sender, which happens once the relay worker
            //     exits.
            // The reaper joins all of them (bounded — see `spawn_reaper`) so no
            // thread/fd leaks, while the actor call path never blocks.
            Self::spawn_reaper(
                session.handshake_thread,
                session.dispatcher_thread,
                relay_dispatcher,
            );
        }
    }

    /// Background reaper: joins a cancelled session's worker handles off the
    /// actor / capability call path. Spawned detached by [`cancel`]; the caller
    /// of `cancel()` returns immediately while this thread reclaims the handles.
    /// A no-op (no thread spawned) when there is nothing to reap.
    ///
    /// Joins are **bounded** ([`REAP_JOIN_BUDGET`]) and abandoned on timeout:
    /// the relay worker's connect path has two steps the in-`nmp-network` connect
    /// timeout does not cover — DNS resolution and the TLS/HTTP upgrade read — so
    /// a worker wedged there might not observe `WorkerCmd::Shutdown` until the OS
    /// resolver / socket times out. Bounding the join guarantees the reaper
    /// itself always terminates rather than living forever; the abandoned worker
    /// still self-exits when its own OS-level timeout fires (it is parked in a
    /// syscall, not spinning), and the relay client's `recv()` then resolves and
    /// that thread exits too. The residual is at most one worker transiently
    /// outliving the reaper while parked in a connect syscall — never an
    /// unbounded busy thread and never a blocked actor.
    fn spawn_reaper(
        handshake_thread: Option<JoinHandle<()>>,
        dispatcher_thread: Option<JoinHandle<()>>,
        relay_dispatcher: Option<JoinHandle<()>>,
    ) {
        if handshake_thread.is_none()
            && dispatcher_thread.is_none()
            && relay_dispatcher.is_none()
        {
            return;
        }
        // Detached: we never join the reaper itself. Naming it aids leak
        // diagnosis in a thread dump.
        let _ = std::thread::Builder::new()
            .name("nmp-broker-cancel-reaper".to_string())
            .spawn(move || {
                // Join the relay client's dispatcher first: it holds the relay's
                // `inbound_tx` event-callback clone (so it gates the broker
                // dispatcher's `recv`) and self-exits once the Pool translator
                // drops its sender, after the relay worker exits.
                Self::reap_join(relay_dispatcher);
                // Then the handshake thread (drops its own `inbound_tx`).
                Self::reap_join(handshake_thread);
                // Finally the broker inbound dispatcher: by now every
                // `inbound_tx` sender is dropped, so its `recv()` has returned
                // `Err` and this join is immediate.
                Self::reap_join(dispatcher_thread);
            });
    }

    /// Join `handle` with a bounded budget; abandon (return) on timeout so the
    /// reaper can never block forever on a worker wedged in an unbounded connect
    /// syscall. Blocks (no busy-wait): a short-lived helper thread owns the
    /// blocking `join()` and signals completion on a channel the reaper waits on
    /// with [`REAP_JOIN_BUDGET`]. If the budget elapses the helper is detached —
    /// it self-completes when the underlying thread eventually exits, so nothing
    /// leaks unboundedly and nothing spins.
    fn reap_join(handle: Option<JoinHandle<()>>) {
        let Some(handle) = handle else { return };
        let (done_tx, done_rx) = mpsc::channel::<()>();
        // Helper thread owns the blocking join. Detached: if it outlives the
        // budget it finishes on its own when the joined thread exits.
        let _ = std::thread::Builder::new()
            .name("nmp-broker-reap-join".to_string())
            .spawn(move || {
                let _ = handle.join();
                let _ = done_tx.send(());
            });
        // Block (no polling) for the join to complete, up to the budget.
        let _ = done_rx.recv_timeout(REAP_JOIN_BUDGET);
    }

    /// Body of the per-handshake worker thread. Outline:
    /// 1. Parse the URI (already shape-validated by the host, but we
    ///    re-parse here for the typed `BunkerUri`).
    /// 2. Connect to the first relay (cycle through if it fails).
    /// 3. Subscribe to inbound kind:24133 events.
    /// 4. Drive the connect → get_public_key state machine.
    /// 5. Construct `Nip46Signer`, emit `SignerReady`, and emit the terminal
    ///    `"ready"` progress snapshot.
    fn run_handshake_thread(self: Arc<Self>, uri_str: String, cancel: Arc<AtomicBool>) {
        let bunker_uri = match parse_bunker_uri(&uri_str) {
            Ok(u) => u,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("parse bunker uri: {e}")));
                return;
            }
        };

        // Local ephemeral keys; the bunker addresses RPC responses to this.
        let local_keys = Keys::generate();
        let remote_pubkey = match PublicKey::from_hex(&bunker_uri.remote_pubkey_hex) {
            Ok(pk) => pk,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("invalid remote pubkey: {e}")));
                return;
            }
        };
        let handle = match Nip46SignerHandle::from_bunker_uri_with_local_key(
            &uri_str,
            local_keys.secret_key().clone(),
        ) {
            Ok(h) => h,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("handle from uri: {e}")));
                return;
            }
        };

        // (inbound_tx, inbound_rx) — the relay client pushes raw event JSON
        // values on the tx; both the handshake state machine and the
        // steady-state transport drain on the rx. We split the dispatch
        // logic between two consumers via a fan-out: during handshake the
        // handshake function owns the receiver; afterwards we re-tap the
        // event callback to route directly to the transport.
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();
        let inbound_tx_for_cb = inbound_tx.clone();
        let event_cb: EventCallback = Arc::new(move |event| {
            // Best-effort: if the receiver is dropped (broker cancelled),
            // silently drop the event.
            let _ = inbound_tx_for_cb.send(event);
        });

        // Dial the first relay. Cycle through on failure.
        let mut relay_result: Option<Arc<dyn RelayClient>> = None;
        let mut last_err: Option<String> = None;
        let conn_state_cb = self.make_connection_state_callback();
        for url in &bunker_uri.relays {
            // Acquire pairs with the Release store in `cancel()` (cross-thread
            // happens-before; load-bearing on ARM — iOS/Android).
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                self.emit_progress("failed", Some("cancelled"));
                return;
            }
            self.emit_progress("connecting", Some(&format!("dialing {url}")));
            match TungsteniteRelayClient::connect(
                url,
                Arc::clone(&event_cb),
                Some(Arc::clone(&conn_state_cb)),
            ) {
                Ok(client) => {
                    relay_result = Some(Arc::new(client) as Arc<dyn RelayClient>);
                    break;
                }
                Err(e) => {
                    last_err = Some(e.to_string());
                }
            }
        }
        let Some(relay) = relay_result else {
            self.emit_progress(
                "failed",
                Some(&format!(
                    "could not connect to any bunker relay: {}",
                    last_err.unwrap_or_else(|| "unknown".to_string())
                )),
            );
            return;
        };

        // Subscribe (REQ). Use the dedicated `subscribe()` method so the
        // relay client remembers the frame and replays it after every
        // reconnect — V-14. A plain `send()` would be lost the moment the
        // socket flaps, leaving the broker with a connected transport that
        // delivers no events.
        let req_frame = build_req_frame(BUNKER_SUB_ID, &local_keys.public_key().to_hex());
        if let Err(e) = relay.subscribe(req_frame) {
            self.emit_progress("failed", Some(&format!("subscribe: {e}")));
            return;
        }

        // Build the transport before the signer — the signer takes `Arc<dyn
        // Nip46Transport>` and the transport holds a `Weak<Nip46Signer>`
        // which we'll bind once we construct the signer.
        let transport = BrokerTransport::new(Arc::clone(&relay), local_keys.clone(), remote_pubkey);

        // Install the live session entry (replacing the placeholder).
        self.install_session(Arc::clone(&relay), Arc::clone(&transport));

        // Run the handshake.
        let mut progress_emitter = |stage: &str, msg: Option<&str>| {
            self.emit_progress(stage, msg);
        };
        let outcome = match run_handshake(
            relay.as_ref(),
            &inbound_rx,
            &local_keys,
            remote_pubkey,
            bunker_uri.secret.as_deref().map(String::as_str),
            bunker_uri.permissions.as_deref(),
            &cancel,
            &mut progress_emitter,
        ) {
            Ok(o) => o,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("{e}")));
                return;
            }
        };

        self.complete_handshake(handle, transport, inbound_rx, outcome);
    }

    /// Replace the placeholder session entry with the real relay/transport.
    fn install_session(&self, relay: Arc<dyn RelayClient>, transport: Arc<BrokerTransport>) {
        if let Ok(mut guard) = self.active.lock() {
            if let Some(session) = guard.as_mut() {
                session.relay = relay;
                session.transport = transport;
            }
        }
    }

    /// Construct the `Nip46Signer`, emit it to the host, drain inbound
    /// events going forward by routing them directly to the transport.
    fn complete_handshake(
        self: &Arc<Self>,
        handle: Nip46SignerHandle,
        transport: Arc<BrokerTransport>,
        inbound_rx: mpsc::Receiver<Value>,
        outcome: HandshakeOutcome,
    ) {
        let user_pubkey = match PublicKey::from_hex(&outcome.user_pubkey_hex) {
            Ok(pk) => pk,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("user pubkey decode: {e}")));
                return;
            }
        };
        // `Nip46SignerHandle::complete` is generic over `T: Nip46Transport`
        // (not `dyn` — `T` must be `Sized`); pass the concrete
        // `Arc<BrokerTransport>` directly. The signer will erase the type
        // internally as `Arc<dyn Nip46Transport>`.
        let signer = Arc::new(handle.complete(Arc::clone(&transport), user_pubkey));
        self.install_completed_signer(signer, transport, inbound_rx);
    }

    fn install_completed_signer(
        self: &Arc<Self>,
        signer: Arc<Nip46Signer>,
        transport: Arc<BrokerTransport>,
        inbound_rx: mpsc::Receiver<Value>,
    ) {
        transport.bind_signer(&signer);

        // ADR-0050 §D3b: route this session's steady-state RPC replies through
        // the completion sink (host → `DeliverSignerResponse`) rather than the
        // dispatcher thread. Bound here so a session that completes before the
        // host installs the sink still picks it up on the next install; if the
        // sink is unset the transport drops replies to a clean op timeout (D6).
        if let Ok(slot) = self.completion_sink.lock() {
            if let Some(sink) = slot.as_ref() {
                transport.bind_completion_sink(Arc::clone(sink));
            }
        }

        // Spawn the inbound dispatcher: route remaining events to the
        // transport for steady-state RPC response delivery. The thread exits
        // on its own when every `inbound_tx` sender drops — the handshake
        // thread's original sender (dropped when that thread returns) and the
        // relay client's event-callback clone (dropped when `relay.shutdown()`
        // joins the relay's own dispatcher). `cancel()` does both (the signal),
        // then hands the handle we stash below to the background reaper, so the
        // thread can never outlive the session yet never blocks the caller
        // (defect: leaked dispatcher thread under rapid reconnects).
        let transport_for_dispatch = Arc::clone(&transport);
        let dispatcher_thread = std::thread::Builder::new()
            .name("nmp-broker-inbound-dispatch".to_string())
            .spawn(move || {
                while let Ok(event) = inbound_rx.recv() {
                    transport_for_dispatch.dispatch_inbound(&event);
                }
            })
            .ok();

        // Stash the signer AND the dispatcher join handle on the active
        // session so cancel() can tear both down deterministically even after
        // the host adapter receives its own strong reference to the signer.
        let mut orphaned_dispatcher = dispatcher_thread;
        if let Ok(mut guard) = self.active.lock() {
            if let Some(session) = guard.as_mut() {
                if let Ok(mut slot) = session.signer.lock() {
                    *slot = Some(Arc::clone(&signer));
                }
                session.dispatcher_thread = orphaned_dispatcher.take();
            }
        }
        // Race guard: if the session was already taken by a concurrent
        // `cancel()` (so the stash above found no session), hand the dispatcher
        // to the background reaper rather than detaching it or joining it here.
        // By this point `cancel()` has dropped the relay's event callback, so
        // the dispatcher's `inbound_rx.recv()` has already (or will shortly)
        // return `Err`; the reaper joins it off-path — never a hang, never a
        // leaked thread. This keeps every join uniformly off the call path.
        if orphaned_dispatcher.is_some() {
            Self::spawn_reaper(None, orphaned_dispatcher, None);
        }

        self.emit(BrokerEvent::SignerReady {
            signer: Arc::clone(&signer),
        });

        // `"ready"` is the broker's terminal success signal. Observers that
        // also watch for a new `signer_kind == "nip46"` account row can drop
        // their progress UI as soon as the row appears — no Rust-side `"idle"`
        // emission is needed. A delayed `"idle"` would be a D8 violation
        // (timer-driven control flow); presentation lifecycle belongs to the
        // UI layer, which can run its own animation if a lingering "Connected"
        // card is desired.
        self.emit_progress("ready", Some("Bunker connected"));
    }

    fn emit_progress(&self, stage: &str, message: Option<&str>) {
        self.emit(BrokerEvent::Progress {
            stage: stage.to_string(),
            message: message.map(str::to_string),
        });
    }

    fn emit_connection_state(&self, state: &str, reason: Option<&str>) {
        self.emit(BrokerEvent::ConnectionStateChanged {
            state: state.to_string(),
            reason: reason.map(str::to_string),
        });
    }

    /// Build the [`crate::relay_client::ConnectionStateCallback`] that routes
    /// relay-layer lifecycle events back through `emit_connection_state`. Called
    /// once per dial attempt so the broker — not the dispatcher thread — owns
    /// the `Arc<Self>` reference used for the callback.
    fn make_connection_state_callback(
        self: &Arc<Self>,
    ) -> crate::relay_client::ConnectionStateCallback {
        let me = Arc::clone(self);
        Arc::new(move |state: &str, reason: Option<&str>| {
            me.emit_connection_state(state, reason);
        })
    }

    fn emit(&self, event: BrokerEvent) {
        (self.events)(event);
    }
}

impl std::fmt::Debug for BunkerBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BunkerBroker").finish_non_exhaustive()
    }
}

/// Placeholder relay client used while a session entry is being constructed.
/// All operations are no-ops; replaced by the real `TungsteniteRelayClient`
/// once the worker thread connects.
#[derive(Debug)]
struct NoopRelay;
impl RelayClient for NoopRelay {
    fn send(&self, _frame: String) -> Result<(), crate::relay_client::RelayError> {
        // The worker swaps this placeholder out for the real transport once
        // the relay socket is up. If `send` is reached while `NoopRelay` is
        // still installed, the handshake raced ahead of the connection —
        // surface that as an error instead of silently dropping the frame
        // (a dropped sign request must never be reported as success).
        Err(crate::relay_client::RelayError::Disconnected)
    }
    fn shutdown(&self) {}
}
