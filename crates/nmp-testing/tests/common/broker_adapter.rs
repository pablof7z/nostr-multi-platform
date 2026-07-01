//! Test-only actor-lane adapter for NIP-46 integration tests.
//!
//! Replaces the deleted `BunkerBroker` (nmp-signer-broker) with a thin test
//! harness that drives NIP-46 through the same actor-lane runtime
//! (`nmp-nip46-runtime`) that production code uses.
//!
//! ## Thread model
//!
//! A background pump thread bridges two worlds:
//!
//! - **Outbound** (`internal_rx` → Pool): `ActorLaneTransport` posts
//!   `EnqueueOutbound { relay_url, text }` to an internal `CommandSender`; the
//!   pump reads it and calls `pool.send(handle, WireFrame::Text(text))`.
//! - **Inbound** (Pool → runtime → external `tx`): `PoolEvent::Frame` is
//!   dispatched to `Nip46Runtime::on_relay_text`.  The runtime returns either
//!   handshake effects (e.g. `SignerReady`) or a steady-state delivery body:
//!   - `SignerReady` → build `Nip46Signer` → `external_tx.add_signer(...)`.
//!   - delivery body → `external_tx.deliver_signer_response(body)`.
//!   - `Progress` effects → `external_tx.bunker_handshake_progress(...)`.
//!
//! The external `tx` is the test's actor-channel sender (the same one passed to
//! `broker_for_actor`).  Tests read from the receiver side (`actor_rx`) for
//! `AddSigner`, `DeliverSignerResponse`, and progress events — identical to how
//! the real actor reads these commands in production.
//!
//! ## Preserved API
//!
//! The public surface is identical to the deleted broker adapter:
//! - [`broker_for_actor`] — constructs an `Arc<ActorLaneAdapter>`.
//! - `start_handshake(bunker_uri)` — parse + connect + init handshake.
//! - `start_nostrconnect_handshake(relay_url, perms)` — generate + return URI.
//! - `cancel()` — clear runtime + close pool connections.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nmp_core::{ActorMail, CommandSender, SignerSource};
use nmp_network::pool::{Pool, PoolConfig, PoolEvent, RelayHandle, WireFrame};
use nmp_network::role::RelayRole;
use nmp_nip46::{Effect, SignerReady};
use nmp_nip46_runtime::transport::ActorLaneTransport;
use nmp_nip46_runtime::{
    cancel_nip46_session, init_bunker, init_nostrconnect, new_nip46_runtime_handle,
    record_signer_ready, Nip46RuntimeHandle,
};
use nmp_signer_iface::RemoteSignerHandle;
use nmp_signers::{parse_bunker_uri, Nip46SignerHandle};
use nostr::{Keys, PublicKey};

mod pump;

/// Test-only NIP-46 actor-lane adapter.
///
/// Maintains a `Pool` for the signer relay connection and a `Nip46RuntimeHandle`
/// for the session state machine.  The background pump thread translates pool
/// events into actor commands on the external `CommandSender` (the test's
/// actor-channel sender).
pub struct ActorLaneAdapter {
    runtime: Nip46RuntimeHandle,
    pool: Arc<Pool>,
    /// relay_url → relay handle.
    url_to_handle: Arc<Mutex<HashMap<String, RelayHandle>>>,
    /// Internal sender for `ActorLaneTransport` (routes `EnqueueOutbound` to pool).
    internal_tx: CommandSender,
    /// External sender for the test's actor channel.
    external_tx: CommandSender,
}

/// Construct an actor-lane adapter that routes actor commands to `tx`.
///
/// The returned `Arc<ActorLaneAdapter>` exposes the same API as the deleted
/// `BunkerBroker`-based adapter:
/// - `start_handshake(bunker_uri)` — start a bunker:// session.
/// - `start_nostrconnect_handshake(relay_url, perms)` — start nostrconnect.
/// - `cancel()` — cancel + teardown.
pub fn broker_for_actor(tx: CommandSender) -> Arc<ActorLaneAdapter> {
    let (internal_tx_raw, internal_rx) = std::sync::mpsc::channel::<ActorMail>();
    let (pool_tx, pool_rx) = std::sync::mpsc::channel::<PoolEvent>();
    let pool = Arc::new(Pool::new(
        PoolConfig {
            default_role: RelayRole::Signer,
            ..Default::default()
        },
        pool_tx,
    ));
    let runtime = new_nip46_runtime_handle();
    let internal_tx = CommandSender::new(internal_tx_raw);
    let url_to_handle: Arc<Mutex<HashMap<String, RelayHandle>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let handle_to_url: Arc<Mutex<HashMap<RelayHandle, String>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Spawn pump thread.
    let pool_for_pump = Arc::clone(&pool);
    let runtime_for_pump = Arc::clone(&runtime);
    let url_to_handle_for_pump = Arc::clone(&url_to_handle);
    let handle_to_url_for_pump = Arc::clone(&handle_to_url);
    let external_tx_for_pump = tx.clone();
    let internal_tx_for_pump = internal_tx.clone();
    std::thread::spawn(move || {
        pump::pump_loop(
            pool_for_pump,
            pool_rx,
            internal_rx,
            runtime_for_pump,
            url_to_handle_for_pump,
            handle_to_url_for_pump,
            external_tx_for_pump,
            internal_tx_for_pump,
        );
    });

    Arc::new(ActorLaneAdapter {
        runtime,
        pool,
        url_to_handle,
        internal_tx,
        external_tx: tx,
    })
}

impl ActorLaneAdapter {
    /// Start a `bunker://` handshake. Connects to each relay in the URI, seeds
    /// the runtime, and sends the initial REQ + connect RPC to the pool.
    pub fn start_handshake(&self, bunker_uri: String) {
        let parsed = match parse_bunker_uri(&bunker_uri) {
            Ok(p) => p,
            Err(e) => {
                self.external_tx.bunker_handshake_progress(
                    "failed".to_string(),
                    None,
                    Some(format!("invalid bunker URI: {e:?}")),
                );
                return;
            }
        };
        let local_keys = Keys::generate();
        let sub_id = format!("nip46-test-{}", &local_keys.public_key().to_hex()[..8]);
        let remote_pubkey = match PublicKey::from_hex(&parsed.remote_pubkey_hex) {
            Ok(pk) => pk,
            Err(e) => {
                self.external_tx.bunker_handshake_progress(
                    "failed".to_string(),
                    None,
                    Some(format!("bad remote pubkey: {e}")),
                );
                return;
            }
        };
        let relay_urls = parsed.relays.clone();
        let secret: Option<String> = parsed.secret.as_ref().map(|zs| zs.as_str().to_string());
        let perms = parsed.permissions.clone();
        let now = now_unix_secs();

        // Open pool connections.
        for relay_url in &relay_urls {
            let h = self
                .pool
                .ensure_open_with_role(relay_url, RelayRole::Signer);
            self.url_to_handle
                .lock()
                .unwrap()
                .insert(relay_url.clone(), h);
        }

        // Seed the runtime.
        let effects = match init_bunker(
            &self.runtime,
            sub_id,
            local_keys,
            remote_pubkey,
            relay_urls,
            secret.as_deref(),
            perms.as_deref(),
            now,
        ) {
            Ok(effects) => effects,
            Err(e) => {
                self.external_tx.bunker_handshake_progress(
                    "failed".to_string(),
                    None,
                    Some(format!("init_bunker error: {e}")),
                );
                return;
            }
        };

        // Send initial effects directly to the pool (pool buffers until connected).
        self.send_effects_to_pool(&effects);
    }

    /// Start a `nostrconnect://` handshake. Returns the URI the client should
    /// display for the bunker to scan. Connects to `relay_url` and seeds the
    /// runtime with a freshly generated ephemeral key pair + random secret.
    pub fn start_nostrconnect_handshake(&self, relay_url: String, perms: Option<String>) -> String {
        let local_keys = Keys::generate();
        let sub_id = format!("nip46-nc-{}", &local_keys.public_key().to_hex()[..8]);
        let expected_secret: String = Keys::generate().public_key().to_hex()[..16].to_string();
        let now = now_unix_secs();

        // Open pool connection.
        let h = self
            .pool
            .ensure_open_with_role(&relay_url, RelayRole::Signer);
        self.url_to_handle
            .lock()
            .unwrap()
            .insert(relay_url.clone(), h);

        match init_nostrconnect(
            &self.runtime,
            sub_id,
            local_keys,
            relay_url,
            expected_secret,
            perms,
            "nmp-test",
            now,
        ) {
            Ok((uri, effects)) => {
                self.send_effects_to_pool(&effects);
                uri
            }
            Err(e) => {
                self.external_tx.bunker_handshake_progress(
                    "failed".to_string(),
                    None,
                    Some(format!("init_nostrconnect error: {e}")),
                );
                String::new()
            }
        }
    }

    /// Cancel the active NIP-46 session. Clears the runtime + closes pool
    /// connections. Idempotent.
    pub fn cancel(&self) {
        cancel_nip46_session(&self.runtime, &self.external_tx);
    }

    /// Send initial effects (Subscribe/SendFrame) directly to the pool.
    fn send_effects_to_pool(&self, effects: &[Effect]) {
        let url_to_handle = self.url_to_handle.lock().unwrap();
        for effect in effects {
            match effect {
                Effect::Subscribe { relay_url, frame }
                | Effect::SendFrame {
                    relay_url,
                    text: frame,
                } => {
                    if let Some(&h) = url_to_handle.get(relay_url) {
                        self.pool.send(h, WireFrame::Text(frame.clone()));
                    }
                }
                _ => {}
            }
        }
    }
}

// ─── SignerReady handler ──────────────────────────────────────────────────────

/// Build a `Nip46Signer` from a `SignerReady` event and post `AddSigner`.
///
/// Mirrors `Nip46Interceptor::handle_signer_ready` but without the kernel
/// context; instead posts to the external `CommandSender` (the test's actor
/// inbox).
fn handle_signer_ready(
    ready: SignerReady,
    runtime: &Nip46RuntimeHandle,
    internal_tx: &CommandSender,
    external_tx: &CommandSender,
) {
    let remote_signer_pubkey = match PublicKey::from_hex(&ready.remote_signer_pubkey_hex) {
        Ok(pk) => pk,
        Err(e) => {
            external_tx.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some(format!("bad remote signer pubkey in SignerReady: {e}")),
            );
            return;
        }
    };
    let user_pubkey = match PublicKey::from_hex(&ready.user_pubkey_hex) {
        Ok(pk) => pk,
        Err(e) => {
            external_tx.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some(format!("bad user pubkey in SignerReady: {e}")),
            );
            return;
        }
    };

    // Persist the learned remote signer pubkey (BLOCKER 1).
    record_signer_ready(runtime, remote_signer_pubkey);

    // Read session params from the runtime.
    let (relay_urls, local_keys) = {
        let guard = runtime.lock().expect("runtime lock in handle_signer_ready");
        let Some(rt) = guard.as_ref() else { return };
        (rt.relay_urls().to_vec(), rt.local_keys().clone())
    };

    // Build the multi-relay transport (routes outbound RPCs to internal_tx).
    let transport = ActorLaneTransport::new_multi(
        internal_tx.clone(),
        local_keys.clone(),
        remote_signer_pubkey,
        relay_urls.clone(),
    );

    // Build a synthetic bunker:// URI so Nip46SignerHandle parses the remote
    // pubkey and relay list.
    let relay_params: String = relay_urls
        .iter()
        .map(|u| format!("&relay={}", nmp_nip46::percent_encode_query_value(u)))
        .collect();
    let synthetic_uri = format!(
        "bunker://{}?{}",
        remote_signer_pubkey.to_hex(),
        relay_params.trim_start_matches('&'),
    );

    let signer_handle = match Nip46SignerHandle::from_bunker_uri_with_local_key(
        &synthetic_uri,
        local_keys.secret_key().clone(),
    ) {
        Ok(h) => h,
        Err(e) => {
            external_tx.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some(format!("signer handle build failed: {e}")),
            );
            return;
        }
    };

    let signer = std::sync::Arc::new(signer_handle.complete(Arc::new(transport), user_pubkey));

    // Progress + connection state (V-14).
    external_tx.bunker_handshake_progress(
        "ready".to_string(),
        None,
        Some("NIP-46 signer ready".to_string()),
    );
    external_tx.bunker_connection_state_changed("connected".to_string(), None);

    // Hand off to the test's actor inbox.
    let source = SignerSource::RemoteHandle(Box::new(ArcRemoteSigner(signer)));
    external_tx.add_signer(source, true);
}

// ─── ArcRemoteSigner ─────────────────────────────────────────────────────────

/// `Arc<Nip46Signer>` wrapper implementing `RemoteSignerHandle`.
///
/// Required because `sign()` parks the operation in `Nip46Signer::pending` and
/// `deliver_response()` resolves it — both sides need the SAME instance via a
/// shared `Arc`.  `Box<dyn RemoteSignerHandle>` in `AddSigner` implies `'static`
/// ownership, so we wrap in Arc and forward.
#[derive(Debug)]
struct ArcRemoteSigner(Arc<nmp_signers::Nip46Signer>);

impl RemoteSignerHandle for ArcRemoteSigner {
    fn pubkey_hex(&self) -> String {
        RemoteSignerHandle::pubkey_hex(&*self.0)
    }

    fn signer_kind(&self) -> &'static str {
        RemoteSignerHandle::signer_kind(&*self.0)
    }

    fn persistence_payload_json(&self) -> Option<String> {
        RemoteSignerHandle::persistence_payload_json(&*self.0)
    }

    fn sign(
        &self,
        unsigned: &nmp_signer_iface::UnsignedEvent,
    ) -> nmp_signer_iface::SignerOp<nmp_signer_iface::SignedEvent> {
        RemoteSignerHandle::sign(&*self.0, unsigned)
    }

    fn nip44_encrypt(
        &self,
        recipient_pubkey: &str,
        plaintext: &str,
    ) -> nmp_signer_iface::SignerOp<String> {
        RemoteSignerHandle::nip44_encrypt(&*self.0, recipient_pubkey, plaintext)
    }

    fn nip44_decrypt(
        &self,
        sender_pubkey: &str,
        ciphertext: &str,
    ) -> nmp_signer_iface::SignerOp<String> {
        RemoteSignerHandle::nip44_decrypt(&*self.0, sender_pubkey, ciphertext)
    }

    fn nip44_decrypt_session_begin(
        &self,
        request: nmp_signer_iface::Nip44DecryptSessionBeginRequest,
    ) -> nmp_signer_iface::SignerOp<nmp_signer_iface::Nip44DecryptSessionGrant> {
        RemoteSignerHandle::nip44_decrypt_session_begin(&*self.0, request)
    }

    fn nip44_decrypt_batch(
        &self,
        request: nmp_signer_iface::Nip44DecryptBatchRequest,
    ) -> nmp_signer_iface::SignerOp<nmp_signer_iface::Nip44DecryptBatchResult> {
        RemoteSignerHandle::nip44_decrypt_batch(&*self.0, request)
    }

    fn nip44_decrypt_session_end(
        &self,
        request: nmp_signer_iface::Nip44DecryptSessionEndRequest,
    ) -> nmp_signer_iface::SignerOp<bool> {
        RemoteSignerHandle::nip44_decrypt_session_end(&*self.0, request)
    }

    fn deliver_response(&self, response_json: &str) {
        self.0.ingest_rpc_response(response_json);
    }

    fn disconnect(&self) {
        self.0.drain_pending_with_error("signer disconnected");
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
