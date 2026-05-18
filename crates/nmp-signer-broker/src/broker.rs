//! `BunkerBroker` — top-level coordinator.
//!
//! Owns:
//! - A clone of the actor command sender (`Sender<ActorCommand>`) for pushing
//!   `BunkerHandshakeProgress` and `AddRemoteSigner` to the kernel.
//! - At most one `ActiveSession` (relay client + transport + handshake
//!   thread). MVP supports a single concurrent bunker; a follow-up can key a
//!   `HashMap<bunker_url, ActiveSession>`.
//!
//! Lifecycle:
//! - `start_handshake(uri)` validates the URI, opens a relay client,
//!   subscribes to inbound responses, spawns a worker thread that drives the
//!   handshake state machine, and reports progress to the actor.
//! - `cancel()` flips the active session's `AtomicBool` cancel flag and
//!   tears down the relay client. Idempotent.
//!
//! Threading: every method here is non-blocking from the caller's POV. The
//! actual relay I/O and handshake protocol runs on a dedicated worker thread
//! per call; the broker keeps the join handle so it can be cleanly torn
//! down.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use nmp_core::substrate::UnsignedEvent;
use nmp_core::{ActorCommand, RemoteSignerHandle};
use nmp_signer_iface::SignerOp;
use nmp_signers::{parse_bunker_uri, Nip46Signer, Nip46SignerHandle};
use nostr::{Keys, PublicKey};
use serde_json::Value;

use crate::handshake::{
    build_req_frame, run_handshake, run_nostrconnect_handshake, HandshakeOutcome,
};
use crate::relay_client::{EventCallback, RelayClient, TungsteniteRelayClient};
use crate::transport::BrokerTransport;

/// Subscription id used for the inbound REQ. One per session is enough.
const BUNKER_SUB_ID: &str = "nmp-bunker";

/// Top-level broker. Single instance per `NmpApp`; constructed by
/// `nmp_signer_broker_init`.
pub struct BunkerBroker {
    actor_tx: Sender<ActorCommand>,
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
    /// `cancel` or when the actor removes the account.
    signer: Mutex<Option<Arc<Nip46Signer>>>,
}

impl BunkerBroker {
    /// Construct a new broker. Holds a clone of the actor's command sender;
    /// the actor itself never sees the broker.
    pub fn new(actor_tx: Sender<ActorCommand>) -> Arc<Self> {
        Arc::new(Self {
            actor_tx,
            active: Mutex::new(None),
        })
    }

    /// Begin handshake for a `bunker://` URI. Returns immediately; the
    /// actual work runs on a worker thread. Cancels any prior in-flight
    /// session first (MVP — single-session).
    pub fn start_handshake(self: &Arc<Self>, uri: String) {
        // Cancel any prior session so a re-submit replaces cleanly.
        self.cancel();

        let me = Arc::clone(self);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        let thread = std::thread::spawn(move || me.run_handshake_thread(uri, cancel_for_thread));

        // Stage the session entry without the transport/signer yet — the
        // handshake thread fills those in via `install_session` once the
        // relay client is connected.
        if let Ok(mut guard) = self.active.lock() {
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

    /// Begin the signer-initiated (`nostrconnect://`) handshake. Returns the
    /// `nostrconnect://` URI **immediately** (it embeds the ephemeral client
    /// pubkey + secret). Spawns a worker thread that:
    /// 1. Connects to `relay_url`.
    /// 2. Subscribes (REQ) for inbound kind:24133 events tagged to the
    ///    ephemeral client pubkey.
    /// 3. Waits for the signer app to dial the relay and send a `connect` RPC.
    /// 4. Validates the secret, replies `ack`, sends `get_public_key`.
    /// 5. Ships `AddRemoteSigner` to the actor on success.
    ///
    /// Progress is reported via `BunkerHandshakeProgress` snapshots using the
    /// same stage strings as the `bunker://` path (`"connecting"`,
    /// `"awaiting_pubkey"`, `"ready"`, `"idle"`, `"failed"`).
    ///
    /// Cancels any prior in-flight bunker or nostrconnect session (MVP
    /// single-session contract).
    pub fn start_nostrconnect_handshake(self: &Arc<Self>, relay_url: String) -> String {
        // Cancel any prior session so a re-scan replaces cleanly.
        self.cancel();

        // Generate ephemeral keypair + secret now (must return URI synchronously).
        let local_keys = Keys::generate();
        let pubkey_hex = local_keys.public_key().to_hex();
        // Derive the session secret: first 8 bytes of secret key as hex.
        let secret_bytes = local_keys.secret_key().as_secret_bytes();
        let secret: String = secret_bytes[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();

        // Percent-encode the relay URL for the URI.
        let encoded_relay: String = relay_url
            .bytes()
            .flat_map(|b| match b {
                b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'_'
                | b'.'
                | b'~' => vec![b as char],
                _ => format!("%{b:02X}").chars().collect::<Vec<_>>(),
            })
            .collect();
        let uri = format!(
            "nostrconnect://{pubkey_hex}?relay={encoded_relay}&secret={secret}&name=Chirp&perms=sign_event%3A1%2Csign_event%3A7"
        );

        let me = Arc::clone(self);
        let secret_for_thread = secret.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        let thread = std::thread::spawn(move || {
            me.run_nostrconnect_thread(relay_url, local_keys, secret_for_thread, cancel_for_thread);
        });

        // Stage the session entry (placeholder relay/transport until the thread
        // connects and installs the real ones).
        if let Ok(mut guard) = self.active.lock() {
            *guard = Some(ActiveSession {
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

        uri
    }

    /// Body of the nostrconnect:// worker thread.
    fn run_nostrconnect_thread(
        self: Arc<Self>,
        relay_url: String,
        local_keys: Keys,
        expected_secret: String,
        cancel: Arc<AtomicBool>,
    ) {
        // Set up inbound channel.
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();
        let inbound_tx_for_cb = inbound_tx.clone();
        let event_cb: EventCallback = Arc::new(move |event| {
            let _ = inbound_tx_for_cb.send(event);
        });

        // Connect to the relay.
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            self.emit_progress("failed", Some("cancelled"));
            return;
        }
        self.emit_progress("connecting", Some(&format!("connecting to relay {relay_url}")));
        let relay = match TungsteniteRelayClient::connect(&relay_url, Arc::clone(&event_cb)) {
            Ok(c) => Arc::new(c) as Arc<dyn RelayClient>,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("relay connect failed: {e}")));
                return;
            }
        };

        // Subscribe to inbound kind:24133 events p-tagged to our ephemeral pubkey.
        let req_frame = build_req_frame(BUNKER_SUB_ID, &local_keys.public_key().to_hex());
        if let Err(e) = relay.send(req_frame) {
            self.emit_progress("failed", Some(&format!("REQ subscribe failed: {e}")));
            return;
        }

        // Build a placeholder transport (we don't know the signer pubkey yet).
        let placeholder_transport = BrokerTransport::new(
            Arc::clone(&relay),
            local_keys.clone(),
            // Placeholder — will be replaced once we learn the signer pubkey.
            local_keys.public_key(),
        );
        self.install_session(Arc::clone(&relay), Arc::clone(&placeholder_transport));

        // Run the nostrconnect handshake (signer-initiated).
        let mut progress_emitter = |stage: &str, msg: Option<&str>| {
            self.emit_progress(stage, msg);
        };
        let outcome = match run_nostrconnect_handshake(
            relay.as_ref(),
            &inbound_rx,
            &local_keys,
            &expected_secret,
            &cancel,
            &mut progress_emitter,
        ) {
            Ok(o) => o,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("{e}")));
                return;
            }
        };

        // Build the real transport now that we know the signer pubkey.
        let signer_pk = match PublicKey::from_hex(&outcome.signer_pubkey_hex) {
            Ok(pk) => pk,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("signer pubkey decode: {e}")));
                return;
            }
        };
        let transport = BrokerTransport::new(Arc::clone(&relay), local_keys.clone(), signer_pk);
        self.install_session(Arc::clone(&relay), Arc::clone(&transport));

        // Build a Nip46SignerHandle from the nostrconnect URI. We reuse the
        // bunker:// handle type by constructing a synthetic bunker:// URI —
        // the handle is just a container for local_keys; the transport does the
        // actual routing.
        let synthetic_bunker_uri = format!(
            "bunker://{}?relay={}",
            outcome.signer_pubkey_hex, relay_url
        );
        let handle = match Nip46SignerHandle::from_bunker_uri_with_local_key(
            &synthetic_bunker_uri,
            local_keys.secret_key().clone(),
        ) {
            Ok(h) => h,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("build signer handle: {e}")));
                return;
            }
        };

        self.complete_handshake(handle, transport, inbound_rx, HandshakeOutcome {
            user_pubkey_hex: outcome.user_pubkey_hex,
        });
    }

    /// Cancel the active session if any. Idempotent.
    pub fn cancel(&self) {
        let session = if let Ok(mut guard) = self.active.lock() {
            guard.take()
        } else {
            None
        };
        if let Some(session) = session {
            session
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
            session.relay.shutdown();
            if let Some(handle) = session.handshake_thread {
                // Best-effort join — bound wait so a wedged thread doesn't
                // hang the caller.
                let _ = handle.join();
            }
        }
    }

    /// Body of the per-handshake worker thread. Outline:
    /// 1. Parse the URI (already shape-validated by the actor, but we
    ///    re-parse here for the typed `BunkerUri`).
    /// 2. Connect to the first relay (cycle through if it fails).
    /// 3. Subscribe to inbound kind:24133 events.
    /// 4. Drive the connect → get_public_key state machine.
    /// 5. Construct `Nip46Signer`, ship `AddRemoteSigner` to the actor.
    /// 6. Emit `idle` after a short delay so the UI clears the progress card.
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
        let relay = match relay_result {
            Some(r) => r,
            None => {
                self.emit_progress(
                    "failed",
                    Some(&format!(
                        "could not connect to any bunker relay: {}",
                        last_err.unwrap_or_else(|| "unknown".to_string())
                    )),
                );
                return;
            }
        };

        // Subscribe (REQ).
        let req_frame = build_req_frame(BUNKER_SUB_ID, &local_keys.public_key().to_hex());
        if let Err(e) = relay.send(req_frame) {
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
            bunker_uri.secret.as_deref(),
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

    /// Construct the `Nip46Signer`, ship it to the actor, drain inbound
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
        transport.bind_signer(&signer);

        // Stash the signer on the active session so it stays alive past
        // this function; the actor's `Box<dyn RemoteSignerHandle>` is the
        // primary owner, but we want a second strong ref so cancel() can
        // tear it down deterministically.
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

        // Tell the actor about the new signer. `AddRemoteSigner` consumes a
        // `Box<dyn RemoteSignerHandle>`; we use an `ArcRemoteSigner` wrapper
        // so we can keep our `Arc<Nip46Signer>` alive in `signer` while the
        // actor holds its own boxed view.
        let actor_handle: Box<dyn RemoteSignerHandle> =
            Box::new(ArcRemoteSigner(Arc::clone(&signer)));
        let _ = self.actor_tx.send(ActorCommand::AddRemoteSigner {
            handle: actor_handle,
        });

        self.emit_progress("ready", Some("Bunker connected"));

        // Schedule a delayed `idle` so the Chirp UI clears the progress card
        // after the new account row is visible. Stage 5 auto-dismisses the
        // sheet on a new nip46 account; this clears the snapshot field too.
        let me = Arc::clone(self);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            me.emit_progress("idle", None);
        });
    }

    /// Generate a `nostrconnect://` URI for the QR-code sign-in flow. Encodes
    /// the ephemeral client pubkey and a random secret. The signer app scans
    /// the QR, connects to `relay_url`, and either:
    ///  (a) sends back a `bunker://` URI via the app callback scheme, or
    ///  (b) drives the relay-based NIP-46 handshake (Phase 2).
    ///
    /// Returns the full URI string. Each call generates fresh ephemeral keys.
    pub fn nostrconnect_uri(&self, relay_url: &str) -> String {
        let client_keys = Keys::generate();
        let pubkey_hex = client_keys.public_key().to_hex();
        // Use first 8 bytes of the secret key as the random session secret.
        let secret_bytes = client_keys.secret_key().as_secret_bytes();
        let secret: String = secret_bytes[..8].iter().map(|b| format!("{b:02x}")).collect();
        // Percent-encode the relay URL for use as a query param value.
        let encoded_relay: String = relay_url
            .bytes()
            .flat_map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    vec![b as char]
                }
                _ => format!("%{b:02X}").chars().collect::<Vec<_>>(),
            })
            .collect();
        format!(
            "nostrconnect://{pubkey_hex}?relay={encoded_relay}&secret={secret}&name=Chirp&perms=sign_event%3A1%2Csign_event%3A7"
        )
    }

    fn emit_progress(&self, stage: &str, message: Option<&str>) {
        let _ = self.actor_tx.send(ActorCommand::BunkerHandshakeProgress {
            stage: stage.to_string(),
            message: message.map(str::to_string),
        });
    }
}

impl std::fmt::Debug for BunkerBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BunkerBroker").finish_non_exhaustive()
    }
}

/// Adapter: `Box<dyn RemoteSignerHandle>` from an `Arc<Nip46Signer>`. The
/// broker keeps its own `Arc` (for cancel/teardown); the actor holds this
/// wrapper. Both delegate to the same underlying signer state.
#[derive(Debug)]
struct ArcRemoteSigner(Arc<Nip46Signer>);

impl RemoteSignerHandle for ArcRemoteSigner {
    fn pubkey_hex(&self) -> String {
        self.0.pubkey_hex()
    }
    fn signer_kind(&self) -> &'static str {
        self.0.signer_kind()
    }
    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<nmp_core::substrate::SignedEvent> {
        self.0.sign(unsigned)
    }
    fn deliver_rpc_response(&self, response_json: &str) {
        self.0.deliver_rpc_response(response_json);
    }
}

/// Placeholder relay client used while a session entry is being constructed.
/// All operations are no-ops; replaced by the real `TungsteniteRelayClient`
/// once the worker thread connects.
#[derive(Debug)]
struct NoopRelay;
impl RelayClient for NoopRelay {
    fn send(&self, _frame: String) -> Result<(), crate::relay_client::RelayError> {
        // Silently drop — by the time anyone is calling `send` on the real
        // session, the worker has already swapped this out.
        Ok(())
    }
    fn shutdown(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// Broker construction is cheap and does not touch the network. Smoke
    /// test that `new` builds without panic and `cancel` on an empty broker
    /// is a no-op.
    #[test]
    fn new_and_cancel_are_noops_without_session() {
        let (tx, _rx) = mpsc::channel::<ActorCommand>();
        let broker = BunkerBroker::new(tx);
        broker.cancel(); // no-op
        broker.cancel(); // still no-op
    }

    #[test]
    fn start_handshake_with_invalid_uri_emits_failed_progress() {
        let (tx, rx) = mpsc::channel::<ActorCommand>();
        let broker = BunkerBroker::new(tx);
        broker.start_handshake("not-a-bunker-uri".to_string());
        // The worker thread runs async; poll the receiver for the failed
        // progress event.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut saw_failed = false;
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(ActorCommand::BunkerHandshakeProgress { stage, .. }) if stage == "failed" => {
                    saw_failed = true;
                    break;
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            saw_failed,
            "expected a failed-progress event for invalid URI"
        );
        broker.cancel();
    }
}
