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

mod handshake_thread;
mod nostrconnect;
mod restore;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crossbeam_channel::Sender as CbSender;
use nmp_signers::Nip46Signer;
use nostr::Keys;

use crate::events::{BrokerEvent, BrokerEventHandler};
use crate::relay_client::RelayClient;
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
    /// Monotonic session generation. Bumped on every `start_handshake` and
    /// every `cancel`. Each handshake worker is stamped with the generation it
    /// was spawned under; `install_session` / `install_completed_signer` no-op
    /// if their stamp no longer matches the active session's generation.
    ///
    /// Load-bearing since D4 made `cancel()` detached: a cancelled (old) worker
    /// is no longer drained before a new session is staged, so without this
    /// guard a late `install_*` from the old worker would silently overwrite the
    /// freshly-staged session's relay/transport/signer. The generation makes
    /// every late write from a superseded worker a no-op.
    generation: AtomicU64,
    /// ADR-0050 §D3b completion sink, installed once by the host adapter via
    /// [`BunkerBroker::set_completion_sink`] and bound onto each session's
    /// transport in `install_completed_signer`. `None` until the host installs
    /// it (the host always does at broker init). The broker never names
    /// `ActorCommand` (D0) — it sees only the opaque `Fn(String)`.
    completion_sink: Mutex<Option<crate::CompletionSink>>,
    /// Test-only observer fired by the reaper AFTER all of its `.join()` calls
    /// return. Lets a test prove the reaper actually JOINED (reclaimed) the
    /// handles — distinguishing a real join from a handle that was merely
    /// dropped (which would also let the worker thread exit). Not present in
    /// production builds.
    #[cfg(test)]
    reaper_observer: Mutex<Option<mpsc::Sender<()>>>,
}

struct ActiveSession {
    /// Generation this session was staged under (see [`BunkerBroker::generation`]).
    /// A worker may only mutate this session if its spawn-time stamp equals
    /// this value.
    generation: u64,
    relay: Arc<dyn RelayClient>,
    cancel: Arc<AtomicBool>,
    /// Event-driven cancel wakeup for the handshake (D8 — no polling). The
    /// handshake `select!`s over its inbound channel AND this receiver; `cancel()`
    /// sends `()` here (and drops this sender) so the in-flight handshake wakes
    /// immediately instead of polling the `cancel` `AtomicBool` on a timer. The
    /// `AtomicBool` is retained only for the cheap pre-dial checkpoint loads
    /// before the handshake's blocking waits begin.
    cancel_tx: CbSender<()>,
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
            generation: AtomicU64::new(0),
            completion_sink: Mutex::new(None),
            #[cfg(test)]
            reaper_observer: Mutex::new(None),
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
        // Cancel any prior session so a re-submit replaces cleanly. `cancel()`
        // is now detached (D4): the old worker may still be mid-flight after
        // this returns, so we MUST stamp this new session with a fresh
        // generation that the old worker can never match (see `generation`).
        self.cancel();

        // New generation for this session. `cancel()` above already bumped the
        // generation (invalidating any prior worker); bump again so this
        // session's stamp is strictly newer than anything the just-cancelled
        // worker carries.
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        // Event-driven cancel wakeup (D8). One-shot is enough: cancellation is
        // terminal. The worker takes `cancel_rx`; the session keeps `cancel_tx`.
        let (cancel_tx, cancel_rx) = crossbeam_channel::bounded::<()>(1);
        let me = Arc::clone(self);

        // Spawn under the lock: the worker's first contention point is its
        // own `self.active.lock()` inside `install_session`, which will block
        // until this scope releases the guard. That guarantees the placeholder
        // is staged before the worker can mutate it — closing the race where a
        // fast worker could reach `install_session` before staging and have
        // its real relay/transport silently dropped (since `install_session`
        // is a mutate-if-Some no-op).
        if let Ok(mut guard) = self.active.lock() {
            let thread = std::thread::spawn(move || {
                me.run_handshake_thread(uri, cancel_for_thread, cancel_rx, generation)
            });
            *guard = Some(ActiveSession {
                generation,
                // Placeholder relay reference until the worker swaps it in.
                // We use an `Arc<NoopRelay>` so the field type stays simple.
                relay: Arc::new(NoopRelay) as Arc<dyn RelayClient>,
                cancel,
                cancel_tx,
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
        // Bump the generation so any worker stamped with the session we are
        // about to tear down can no longer install into a future session. This
        // is the correctness guard for the detached teardown: the old worker
        // outlives this call, so a generation it can never match is what stops
        // its late `install_*` from clobbering a subsequently-staged session.
        self.generation.fetch_add(1, Ordering::AcqRel);

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
            // Event-driven cancel wakeup (D8 — no polling). The in-flight
            // handshake `select!`s over its inbound channel AND `cancel_rx`;
            // sending here wakes it immediately. `try_send` is non-blocking (the
            // bounded(1) channel never backs up — cancel is one-shot) and runs
            // on the actor call path, which must not block. Even if the send is
            // dropped, dropping `cancel_tx` with the session disconnects
            // `cancel_rx`, which the handshake `select!` also treats as
            // cancellation — so the wakeup is guaranteed either way.
            let _ = session.cancel_tx.try_send(());
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
            self.spawn_reaper(
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
        &self,
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
        // Test-only: clone the observer so the reaper can signal once it has
        // JOINED all handles. Production builds carry no observer.
        #[cfg(test)]
        let observer = self
            .reaper_observer
            .lock()
            .ok()
            .and_then(|g| g.clone());
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
                // Signal AFTER every join returned — proves the reaper reclaimed
                // (joined) the handles, not merely dropped them.
                #[cfg(test)]
                if let Some(tx) = observer {
                    let _ = tx.send(());
                }
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
