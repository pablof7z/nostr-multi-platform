use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use nmp_signers::{Nip46Signer, SignerPayload};
use nostr::{Keys, PublicKey, SecretKey};
use serde_json::Value;

use super::{ActiveSession, BunkerBroker, NoopRelay, BUNKER_SUB_ID};
use crate::handshake::build_req_frame;
use crate::relay_client::{EventCallback, RelayClient, TungsteniteRelayClient};
use crate::transport::BrokerTransport;

impl BunkerBroker {
    /// Restore an authorized NIP-46 session from the payload persisted by the
    /// Rust actor. This path never asks the user to authorize again.
    pub fn restore_session(self: &Arc<Self>, payload_json: String) {
        self.cancel();

        let me = Arc::clone(self);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);

        // Spawn under the lock so the worker can't reach `install_session`
        // before the placeholder is staged. See `broker.rs::start_handshake`
        // for the full ordering argument.
        if let Ok(mut guard) = self.active.lock() {
            let thread =
                std::thread::spawn(move || me.run_restore_thread(payload_json, cancel_for_thread));
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
    }

    fn run_restore_thread(self: Arc<Self>, payload_json: String, cancel: Arc<AtomicBool>) {
        let payload = match serde_json::from_str::<SignerPayload>(&payload_json) {
            Ok(SignerPayload::Nip46(payload)) => payload,
            Ok(_) => {
                self.emit_progress("failed", Some("stored signer payload is not nip46"));
                return;
            }
            Err(e) => {
                self.emit_progress("failed", Some(&format!("parse signer payload: {e}")));
                return;
            }
        };
        let local_sk = match SecretKey::from_hex(payload.local_secret_hex.as_str()) {
            Ok(sk) => sk,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("stored local key: {e}")));
                return;
            }
        };
        let local_keys = Keys::new(local_sk);
        let remote_pubkey = match PublicKey::from_hex(&payload.remote_pubkey_hex) {
            Ok(pk) => pk,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("stored remote pubkey: {e}")));
                return;
            }
        };
        let Some((relay, inbound_rx)) = self.connect_session(&payload.relays, &local_keys, &cancel) else {
            return;
        };
        let transport = BrokerTransport::new(Arc::clone(&relay), local_keys, remote_pubkey);
        self.install_session(Arc::clone(&relay), Arc::clone(&transport));

        let signer = match Nip46Signer::from_payload(&payload, Arc::clone(&transport)) {
            Ok(signer) => Arc::new(signer),
            Err(e) => {
                self.emit_progress("failed", Some(&format!("restore signer: {e}")));
                return;
            }
        };
        self.install_completed_signer(signer, transport, inbound_rx);
    }

    fn connect_session(
        &self,
        relays: &[String],
        local_keys: &Keys,
        cancel: &AtomicBool,
    ) -> Option<(Arc<dyn RelayClient>, mpsc::Receiver<Value>)> {
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();
        let inbound_tx_for_cb = inbound_tx.clone();
        let event_cb: EventCallback = Arc::new(move |event| {
            let _ = inbound_tx_for_cb.send(event);
        });

        let mut relay_result: Option<Arc<dyn RelayClient>> = None;
        let mut last_err: Option<String> = None;
        for url in relays {
            if cancel.load(Ordering::Relaxed) {
                self.emit_progress("failed", Some("cancelled"));
                return None;
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
            return None;
        };

        // V-14: use `subscribe()` so the REQ is replayed after any
        // transparent reconnect; `send()` would be lost on the first flap.
        let req_frame = build_req_frame(BUNKER_SUB_ID, &local_keys.public_key().to_hex());
        if let Err(e) = relay.subscribe(req_frame) {
            self.emit_progress("failed", Some(&format!("subscribe: {e}")));
            return None;
        }

        Some((relay, inbound_rx))
    }
}
