use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossbeam_channel::Receiver as CbReceiver;
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

        // Fresh generation for this session — strictly newer than anything the
        // just-cancelled (detached) worker carries. See `broker.rs::generation`.
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;

        let me = Arc::clone(self);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        // Restore runs no interactive handshake, but `cancel()` still signals
        // this channel (and drops the sender) on teardown; the dial loop in
        // `connect_session` observes cancellation via the `cancel` AtomicBool
        // checkpoint. Kept for the shared `ActiveSession` shape (D8 — no polling).
        let (cancel_tx, _cancel_rx) = crossbeam_channel::bounded::<()>(1);

        // Spawn under the lock so the worker can't reach `install_session`
        // before the placeholder is staged. See `broker.rs::start_handshake`
        // for the full ordering argument.
        if let Ok(mut guard) = self.active.lock() {
            let thread = std::thread::spawn(move || {
                me.run_restore_thread(payload_json, cancel_for_thread, generation)
            });
            *guard = Some(ActiveSession {
                generation,
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

    fn run_restore_thread(
        self: Arc<Self>,
        payload_json: String,
        cancel: Arc<AtomicBool>,
        generation: u64,
    ) {
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
        // No-op if superseded (detached cancel + newer session staged while we
        // dialed). Tear our own relay down off-path and stop.
        if !self.install_session(generation, Arc::clone(&relay), Arc::clone(&transport)) {
            let relay_dispatcher = relay.signal_shutdown();
            self.spawn_reaper(None, None, relay_dispatcher);
            return;
        }

        let signer = match Nip46Signer::from_payload(&payload, Arc::clone(&transport)) {
            Ok(signer) => Arc::new(signer),
            Err(e) => {
                self.emit_progress("failed", Some(&format!("restore signer: {e}")));
                return;
            }
        };
        self.install_completed_signer(signer, transport, inbound_rx, generation);
    }

    fn connect_session(
        self: &Arc<Self>,
        relays: &[String],
        local_keys: &Keys,
        cancel: &AtomicBool,
    ) -> Option<(Arc<dyn RelayClient>, CbReceiver<Value>)> {
        // Bounded to SIGNER_BROKER_INTAKE_CAP to prevent unbounded growth
        // against a noisy/hostile relay (D5/D8).
        let (inbound_tx, inbound_rx) = crossbeam_channel::bounded::<Value>(crate::SIGNER_BROKER_INTAKE_CAP);
        let inbound_tx_for_cb = inbound_tx.clone();
        let event_cb: EventCallback = Arc::new(move |event| {
            // Non-blocking try-send: on Full, drop the frame and record it.
            // D8: no blocking the relay dispatcher thread.
            match inbound_tx_for_cb.try_send(event) {
                Ok(()) => true,
                Err(crossbeam_channel::TrySendError::Full(_)) => false,
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => false,
            }
        });

        let conn_state_cb = self.make_connection_state_callback();
        let mut relay_result: Option<Arc<dyn RelayClient>> = None;
        let mut last_err: Option<String> = None;
        for url in relays {
            // Acquire pairs with the Release store in `BunkerBroker::cancel()`
            // (cross-thread happens-before; load-bearing on ARM — iOS/Android).
            if cancel.load(Ordering::Acquire) {
                self.emit_progress("failed", Some("cancelled"));
                return None;
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
