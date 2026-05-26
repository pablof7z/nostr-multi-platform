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
//! - `cancel()` flips the active session's `AtomicBool` cancel flag and
//!   tears down the relay client. Idempotent.
//!
//! Threading: every method here is non-blocking from the caller's POV. The
//! actual relay I/O and handshake protocol runs on a dedicated worker thread
//! per call; the broker keeps the join handle so it can be cleanly torn
//! down.

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

/// Top-level broker. Host composition owns the app/actor adapter and passes
/// its event callback here.
pub struct BunkerBroker {
    events: Arc<BrokerEventHandler>,
    active: Mutex<Option<ActiveSession>>,
}

struct ActiveSession {
    relay: Arc<dyn RelayClient>,
    cancel: Arc<AtomicBool>,
    handshake_thread: Option<JoinHandle<()>>,
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
        })
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
    pub fn cancel(&self) {
        let session = if let Ok(mut guard) = self.active.lock() {
            guard.take()
        } else {
            None
        };
        if let Some(session) = session {
            // Drain any in-flight sign requests so callers fail fast instead
            // of waiting out REMOTE_SIGN_TIMEOUT (5s). The signer's pending
            // map still holds the response senders for requests already
            // submitted to the broker; without this they would be orphaned.
            if let Ok(slot) = session.signer.lock() {
                if let Some(signer) = slot.as_ref() {
                    signer.drain_pending_with_error("bunker session cancelled");
                }
            }
            session
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
            session.relay.shutdown();
            if let Some(handle) = session.handshake_thread {
                // Best-effort join. Two distinct threads need to exit:
                //   - The handshake thread observes `cancel` via the
                //     `await_response` recv_timeout loop in handshake.rs;
                //     wakes within ~200ms.
                //   - The relay worker thread observes the
                //     `WorkerCmd::Shutdown` we just sent via
                //     `session.relay.shutdown()`. It exits promptly when
                //     idle or mid-backoff (the readiness poll wakes via the
                //     control-channel mio::Waker; the backoff wait blocks on
                //     a control recv_timeout that drops out on Shutdown).
                // Residual stall window: if the worker is currently inside
                // `tungstenite::connect()` (TCP+TLS+WS handshake), Shutdown
                // sits in the channel until the OS-level connect timeout
                // returns. Shared with `nmp-core::relay_worker`; tracked
                // for resolution by V-13 Stage 1 (shared connection
                // primitive with explicit connect_timeout) in `docs/BACKLOG.md`.
                let _ = handle.join();
            }
        }
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
        for url in &bunker_uri.relays {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                self.emit_progress("failed", Some("cancelled"));
                return;
            }
            self.emit_progress("connecting", Some(&format!("dialing {url}")));
            match TungsteniteRelayClient::connect(url, Arc::clone(&event_cb)) {
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

        // Stash the signer on the active session so cancel() can tear it
        // down deterministically even after the host adapter receives its
        // own strong reference.
        if let Ok(guard) = self.active.lock() {
            if let Some(session) = guard.as_ref() {
                if let Ok(mut slot) = session.signer.lock() {
                    *slot = Some(Arc::clone(&signer));
                }
            }
        }

        // Spawn the inbound dispatcher: route remaining events to the
        // transport for steady-state RPC response delivery.
        let transport_for_dispatch = Arc::clone(&transport);
        std::thread::spawn(move || {
            while let Ok(event) = inbound_rx.recv() {
                transport_for_dispatch.dispatch_inbound(&event);
            }
        });

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
