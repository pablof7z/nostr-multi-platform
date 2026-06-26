use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossbeam_channel::Receiver as CbReceiver;
use nmp_nip46::start_nostrconnect;
use nmp_signers::Nip46SignerHandle;
use nostr::{Keys, PublicKey};
use rand::Rng;

use super::drive::{drive, now_secs, DriveOutcome};
use super::{ActiveSession, BunkerBroker, NoopRelay, BUNKER_SUB_ID};
use crate::relay_client::{RelayClient, TungsteniteRelayClient};
use crate::transport::BrokerTransport;

/// Protocol-neutral `name=` value advertised in the `nostrconnect://` URI.
///
/// D0: a protocol crate must not bake an app brand (e.g. `Chirp`) into a wire
/// string. The `name` field is the human-readable client identifier the remote
/// signer shows the user; the protocol layer reports the substrate's own name
/// and leaves app-specific branding to the app layer.
const NOSTRCONNECT_CLIENT_NAME: &str = "nmp";

impl BunkerBroker {
    /// Begin the signer-initiated (`nostrconnect://`) handshake and return the
    /// URI immediately so native code can render the QR code.
    /// `perms` is the APP-SUPPLIED NIP-46 permission request (#1493 P9), a plain
    /// (NOT percent-encoded) comma-joined perm list such as
    /// `"sign_event:1,sign_event:7"`. When `Some`, it is appended as a
    /// percent-encoded `&perms=` query parameter; when `None`, the parameter is
    /// omitted entirely — the broker (a protocol crate) supplies no default kind
    /// set of its own.
    pub fn start_nostrconnect_handshake(
        self: &Arc<Self>,
        relay_url: String,
        perms: Option<String>,
    ) -> String {
        self.cancel();

        let local_keys = Keys::generate();
        let secret: String = rand::thread_rng()
            .sample_iter(rand::distributions::Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();

        // `start_nostrconnect` builds the URI, state, and start_effects
        // synchronously — no I/O, no clock. The returned URI is forwarded to
        // the caller (QR code rendering) immediately; state + start_effects
        // are forwarded to the worker thread.
        let now = now_secs();
        let (uri, state, start_effects) = start_nostrconnect(
            BUNKER_SUB_ID,
            local_keys.clone(),
            relay_url.clone(),
            secret.clone(),
            perms,
            NOSTRCONNECT_CLIENT_NAME,
            now,
        );

        // Fresh generation for this session — strictly newer than anything the
        // just-cancelled (detached) worker carries. See `broker.rs::generation`.
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;

        let me = Arc::clone(self);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        // Event-driven cancel wakeup (D8 — no polling); see `broker.rs`.
        let (cancel_tx, cancel_rx) = crossbeam_channel::bounded::<()>(1);

        // Spawn under the lock so the worker can't reach `install_session`
        // before the placeholder is staged. See `broker.rs::start_handshake`
        // for the full ordering argument.
        if let Ok(mut guard) = self.active.lock() {
            let thread = std::thread::spawn(move || {
                me.run_nostrconnect_thread(
                    relay_url,
                    local_keys,
                    state,
                    start_effects,
                    cancel_for_thread,
                    cancel_rx,
                    generation,
                );
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

        uri
    }

    fn run_nostrconnect_thread(
        self: Arc<Self>,
        relay_url: String,
        local_keys: Keys,
        mut state: nmp_nip46::SessionState,
        start_effects: Vec<nmp_nip46::Effect>,
        cancel: Arc<AtomicBool>,
        cancel_rx: CbReceiver<()>,
        generation: u64,
    ) {
        let (event_cb, inbound_rx) = self.make_relay_intake();

        // Acquire pairs with the Release store in `BunkerBroker::cancel()`
        // (cross-thread happens-before; load-bearing on ARM — iOS/Android).
        if cancel.load(Ordering::Acquire) {
            self.emit_progress("failed", Some("cancelled"));
            return;
        }
        self.emit_progress(
            "connecting",
            Some(&format!("connecting to relay {relay_url}")),
        );
        let conn_state_cb = self.make_connection_state_callback();
        let relay = match TungsteniteRelayClient::connect(
            &relay_url,
            Arc::clone(&event_cb),
            Some(conn_state_cb),
        ) {
            Ok(c) => Arc::new(c) as Arc<dyn RelayClient>,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("relay connect failed: {e}")));
                return;
            }
        };

        let placeholder_transport = BrokerTransport::new(
            Arc::clone(&relay),
            local_keys.clone(),
            local_keys.public_key(),
        );
        // No-op if superseded; tear our own relay down off-path and stop.
        if !self.install_session(generation, Arc::clone(&relay), Arc::clone(&placeholder_transport))
        {
            let relay_dispatcher = relay.signal_shutdown();
            self.spawn_reaper(None, None, relay_dispatcher);
            return;
        }

        // Drive the reducer to completion. `start_effects` has [Subscribe, Progress]
        // from `start_nostrconnect`; `drive` applies Subscribe → relay.subscribe,
        // Progress → emit_progress, then waits for the signer's connect event.
        let mut progress_emitter = |stage: &str, code: &str, msg: Option<&str>| {
            self.emit_progress_coded(stage, code, msg);
        };
        let sr = match drive(
            &mut state,
            relay.as_ref(),
            start_effects,
            &inbound_rx,
            &cancel_rx,
            &mut progress_emitter,
        ) {
            DriveOutcome::Ready(sr) => sr,
            DriveOutcome::SubscribeFailed(e) => {
                // Byte-identical to the prior nostrconnect worker's subscribe error.
                self.emit_progress("failed", Some(&format!("REQ subscribe failed: {e}")));
                return;
            }
            DriveOutcome::Failed(e) => {
                self.emit_progress("failed", Some(&format!("{e}")));
                return;
            }
            DriveOutcome::Cancelled => {
                self.emit_progress("failed", Some("cancelled"));
                return;
            }
        };

        // The signer's pubkey is now known from the reducer's SignerReady.
        let signer_pk = match PublicKey::from_hex(&sr.remote_signer_pubkey_hex) {
            Ok(pk) => pk,
            Err(e) => {
                self.emit_progress("failed", Some(&format!("signer pubkey decode: {e}")));
                return;
            }
        };
        let transport = BrokerTransport::new(Arc::clone(&relay), local_keys.clone(), signer_pk);
        // No-op if superseded between the placeholder install and now. The relay
        // installed by the placeholder install was already torn down by whoever
        // bumped the generation (they took that session and `signal_shutdown()`-ed
        // it), so we must not touch it here — just stop.
        if !self.install_session(generation, Arc::clone(&relay), Arc::clone(&transport)) {
            return;
        }

        let synthetic_bunker_uri =
            format!("bunker://{}?relay={}", sr.remote_signer_pubkey_hex, relay_url);
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
            sr.user_pubkey_hex,
            generation,
        );
    }
}
