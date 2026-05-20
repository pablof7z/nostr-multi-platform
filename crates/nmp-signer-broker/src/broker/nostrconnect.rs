use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use nmp_signers::Nip46SignerHandle;
use nostr::{Keys, PublicKey};
use rand::Rng;
use serde_json::Value;

use super::{ActiveSession, BunkerBroker, NoopRelay, BUNKER_SUB_ID};
use crate::handshake::{build_req_frame, run_nostrconnect_handshake, HandshakeOutcome};
use crate::relay_client::{EventCallback, RelayClient, TungsteniteRelayClient};
use crate::transport::BrokerTransport;

impl BunkerBroker {
    /// Begin the signer-initiated (`nostrconnect://`) handshake and return the
    /// URI immediately so native code can render the QR code.
    pub fn start_nostrconnect_handshake(self: &Arc<Self>, relay_url: String) -> String {
        self.cancel();

        let local_keys = Keys::generate();
        let pubkey_hex = local_keys.public_key().to_hex();
        let secret: String = rand::thread_rng()
            .sample_iter(rand::distributions::Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        let encoded_relay = percent_encode_query_value(&relay_url);
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

    fn run_nostrconnect_thread(
        self: Arc<Self>,
        relay_url: String,
        local_keys: Keys,
        expected_secret: String,
        cancel: Arc<AtomicBool>,
    ) {
        let (inbound_tx, inbound_rx) = mpsc::channel::<Value>();
        let inbound_tx_for_cb = inbound_tx.clone();
        let event_cb: EventCallback = Arc::new(move |event| {
            let _ = inbound_tx_for_cb.send(event);
        });

        if cancel.load(Ordering::Relaxed) {
            self.emit_progress("failed", Some("cancelled"));
            return;
        }
        self.emit_progress(
            "connecting",
            Some(&format!("connecting to relay {relay_url}")),
        );
        let relay = match TungsteniteRelayClient::connect(&relay_url, Arc::clone(&event_cb)) {
            Ok(c) => Arc::new(c) as Arc<dyn RelayClient>,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("relay connect failed: {e}")));
                return;
            }
        };

        let req_frame = build_req_frame(BUNKER_SUB_ID, &local_keys.public_key().to_hex());
        if let Err(e) = relay.send(req_frame) {
            self.emit_progress("failed", Some(&format!("REQ subscribe failed: {e}")));
            return;
        }

        let placeholder_transport = BrokerTransport::new(
            Arc::clone(&relay),
            local_keys.clone(),
            local_keys.public_key(),
        );
        self.install_session(Arc::clone(&relay), Arc::clone(&placeholder_transport));

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

        let signer_pk = match PublicKey::from_hex(&outcome.signer_pubkey_hex) {
            Ok(pk) => pk,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("signer pubkey decode: {e}")));
                return;
            }
        };
        let transport = BrokerTransport::new(Arc::clone(&relay), local_keys.clone(), signer_pk);
        self.install_session(Arc::clone(&relay), Arc::clone(&transport));

        let synthetic_bunker_uri =
            format!("bunker://{}?relay={}", outcome.signer_pubkey_hex, relay_url);
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

        self.complete_handshake(
            handle,
            transport,
            inbound_rx,
            HandshakeOutcome {
                user_pubkey_hex: outcome.user_pubkey_hex,
            },
        );
    }
}

fn percent_encode_query_value(value: &str) -> String {
    value
        .bytes()
        .flat_map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => vec![b as char],
            _ => format!("%{b:02X}").chars().collect::<Vec<_>>(),
        })
        .collect()
}
