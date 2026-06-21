//! Client-initiated (`bunker://`) handshake worker + the shared session-install
//! helpers.
//!
//! Split out of `broker.rs` to keep that file under the 500-LOC ceiling. These
//! are `impl BunkerBroker` methods like the rest of the broker; they live in a
//! child module so the lifecycle/cancellation core stays readable. The three
//! install helpers are `pub(super)` because the sibling `nostrconnect` and
//! `restore` workers reuse them.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crossbeam_channel::Receiver as CbReceiver;
use nmp_signers::{parse_bunker_uri, Nip46Signer, Nip46SignerHandle};
use nostr::{Keys, PublicKey};
use serde_json::Value;

use super::{BunkerBroker, BUNKER_SUB_ID};
use crate::events::BrokerEvent;
use crate::handshake::{build_req_frame, run_handshake, HandshakeOutcome};
use crate::relay_client::{EventCallback, RelayClient, TungsteniteRelayClient};
use crate::transport::BrokerTransport;

impl BunkerBroker {
    /// Body of the per-handshake worker thread. Outline:
    /// 1. Parse the URI (already shape-validated by the host, but we
    ///    re-parse here for the typed `BunkerUri`).
    /// 2. Connect to the first relay (cycle through if it fails).
    /// 3. Subscribe to inbound kind:24133 events.
    /// 4. Drive the connect → get_public_key state machine.
    /// 5. Construct `Nip46Signer`, emit `SignerReady`, and emit the terminal
    ///    `"ready"` progress snapshot.
    pub(super) fn run_handshake_thread(
        self: Arc<Self>,
        uri_str: String,
        cancel: Arc<AtomicBool>,
        cancel_rx: CbReceiver<()>,
        generation: u64,
    ) {
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
        let (inbound_tx, inbound_rx) = crossbeam_channel::unbounded::<Value>();
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

        // Install the live session entry (replacing the placeholder). No-ops
        // if this worker has been superseded (its generation no longer matches
        // the active session) — e.g. a detached `cancel()` + a new
        // `start_handshake()` ran while we were dialing.
        if !self.install_session(generation, Arc::clone(&relay), Arc::clone(&transport)) {
            // Superseded: our relay/transport must not leak. Tear our own relay
            // down (signal + detached reap), drop everything, and stop — a
            // newer session owns `active` now.
            let relay_dispatcher = relay.signal_shutdown();
            self.spawn_reaper(None, None, relay_dispatcher);
            return;
        }

        // Run the handshake.
        let mut progress_emitter = |stage: &str, code: &str, msg: Option<&str>| {
            self.emit_progress_coded(stage, code, msg);
        };
        let outcome = match run_handshake(
            relay.as_ref(),
            &inbound_rx,
            &cancel_rx,
            &local_keys,
            remote_pubkey,
            bunker_uri.secret.as_deref().map(String::as_str),
            bunker_uri.permissions.as_deref(),
            &mut progress_emitter,
        ) {
            Ok(o) => o,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("{e}")));
                return;
            }
        };

        self.complete_handshake(handle, transport, inbound_rx, outcome, generation);
    }

    /// Replace the placeholder session entry with the real relay/transport.
    ///
    /// Returns `true` iff the install applied. No-ops and returns `false` if
    /// the worker has been superseded — there is no active session, or the
    /// active session's generation differs from `generation` (a detached
    /// `cancel()` and/or a newer `start_handshake()` ran while this worker was
    /// dialing). This is the D4 generation guard: a late write from an old
    /// worker can never overwrite a newer session's relay/transport.
    pub(super) fn install_session(
        &self,
        generation: u64,
        relay: Arc<dyn RelayClient>,
        transport: Arc<BrokerTransport>,
    ) -> bool {
        if let Ok(mut guard) = self.active.lock() {
            if let Some(session) = guard.as_mut() {
                if session.generation == generation {
                    session.relay = relay;
                    session.transport = transport;
                    return true;
                }
            }
        }
        false
    }

    /// Construct the `Nip46Signer`, emit it to the host, drain inbound
    /// events going forward by routing them directly to the transport.
    pub(super) fn complete_handshake(
        self: &Arc<Self>,
        handle: Nip46SignerHandle,
        transport: Arc<BrokerTransport>,
        inbound_rx: CbReceiver<Value>,
        outcome: HandshakeOutcome,
        generation: u64,
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
        self.install_completed_signer(signer, transport, inbound_rx, generation);
    }

    pub(super) fn install_completed_signer(
        self: &Arc<Self>,
        signer: Arc<Nip46Signer>,
        transport: Arc<BrokerTransport>,
        inbound_rx: CbReceiver<Value>,
        generation: u64,
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

        // Stash the signer AND the dispatcher join handle on the active session
        // — but ONLY if this worker still owns `active` (its generation matches
        // the staged session). A detached `cancel()` and/or a newer
        // `start_handshake()` may have run while we completed the handshake; in
        // that case we are superseded and must NOT install our signer (it would
        // clobber the newer session / emit a stale `SignerReady`).
        let mut orphaned_dispatcher = dispatcher_thread;
        let mut installed = false;
        if let Ok(mut guard) = self.active.lock() {
            if let Some(session) = guard.as_mut() {
                if session.generation == generation {
                    if let Ok(mut slot) = session.signer.lock() {
                        *slot = Some(Arc::clone(&signer));
                    }
                    session.dispatcher_thread = orphaned_dispatcher.take();
                    installed = true;
                }
            }
        }
        if !installed {
            // Superseded (or already cancelled). The relay this worker created
            // was installed into the session by `install_session` (generation
            // matched then); whoever bumped the generation since took that
            // session and already `signal_shutdown()`-ed the relay — so we must
            // NOT touch the relay here (no double teardown, no leak). We only
            // own the just-spawned inbound dispatcher: hand it to the reaper.
            // Drain any pending signs the signer accumulated and do NOT emit
            // `SignerReady` / `"ready"` — a newer session (or none) owns the
            // active slot.
            signer.drain_pending_with_error("bunker session superseded");
            self.spawn_reaper(None, orphaned_dispatcher.take(), None);
            return;
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
}
