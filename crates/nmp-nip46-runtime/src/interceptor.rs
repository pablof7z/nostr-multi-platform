//! `Nip46Interceptor` — [`RelayTextInterceptor`] implementation.
//!
//! The interceptor is the actor's inbound gateway for the bunker relay lane.
//! On every text frame from the bunker relay it:
//!
//! 1. Filters by relay URL (ignores frames from non-bunker relays — D6).
//! 2. Drives [`SessionState::on_relay_text`] with the frame and `now_secs`.
//! 3. Translates each returned [`Effect`] into either an outbound message or
//!    an actor command posted via the captured [`CommandSender`]:
//!    - [`Effect::Subscribe`] → register persistent sub + return outbound frame.
//!    - [`Effect::SendFrame`] → return outbound frame (signer RPC).
//!    - [`Effect::Progress`] → `CommandSender::bunker_handshake_progress`.
//!    - [`Effect::SignerReady`] → `CommandSender::add_signer(RemoteHandle)`.
//!    - [`Effect::DeliverResponse`] → `CommandSender::deliver_signer_response`.
//!    - [`Effect::Error`] → `CommandSender::bunker_handshake_progress("failed")`.
//!
//! The `on_idle_tick` hook:
//! - Registers the persistent sub with the kernel on the first tick after
//!   session init (when `persistent_sub_registered == false`).
//! - Drives [`SessionState::tick`] for the 60 s per-step deadline.

use nmp_core::substrate::RelayTextInterceptor;
use nmp_core::{CommandSender, Kernel, OutboundMessage, SignerSource};
use nmp_network::role::RelayRole;
use nmp_nip46::{Effect, SignerReady};
use nmp_signer_iface::{Nip46Rpc, Nip46Transport, RemoteSignerHandle, SignerError, SignerOp};
use nmp_signer_iface::signing::{SignedEvent, UnsignedEvent};
use nostr::PublicKey;

use crate::runtime::Nip46RuntimeHandle;
use crate::transport::ActorLaneTransport;

/// Relay-text interceptor that drives the NIP-46 session state machine.
///
/// Holds a [`Nip46RuntimeHandle`] and a [`CommandSender`] captured at
/// registration time. The interceptor is `Send + Sync`; every operation locks
/// the handle for the minimum duration (effects are collected under the lock,
/// translated outside it).
pub(crate) struct Nip46Interceptor {
    pub(crate) runtime: Nip46RuntimeHandle,
    pub(crate) sender: CommandSender,
}

impl RelayTextInterceptor for Nip46Interceptor {
    fn on_relay_text(
        &self,
        kernel: &mut Kernel,
        relay_url: &str,
        text: &str,
    ) -> Vec<OutboundMessage> {
        let now = kernel.now_secs();

        // ── Phase 1: drive reducer under lock ────────────────────────────
        let effects = {
            let Ok(mut guard) = self.runtime.lock() else {
                return Vec::new();
            };
            let Some(rt) = guard.as_mut() else {
                return Vec::new();
            };
            rt.on_relay_text(relay_url, text, now)
        }; // lock released

        // ── Phase 2: translate effects (kernel + sender, no lock) ─────────
        self.translate_effects(effects, kernel)
    }

    fn on_idle_tick(&self, kernel: &mut Kernel) -> Vec<OutboundMessage> {
        let now = kernel.now_secs();

        // ── Phase 1: tick + collect registration needs under lock ─────────
        let (needs_register, relay_url, sub_id, effects) = {
            let Ok(mut guard) = self.runtime.lock() else {
                return Vec::new();
            };
            let Some(rt) = guard.as_mut() else {
                return Vec::new();
            };

            let needs_reg = !rt.persistent_sub_registered;
            let relay_url = if needs_reg { Some(rt.relay_url.clone()) } else { None };
            let sub_id = if needs_reg { Some(rt.sub_id.clone()) } else { None };
            // Tick for 60 s step timeout.
            let tick_effects = rt.tick(now);

            if needs_reg {
                rt.persistent_sub_registered = true;
            }
            (needs_reg, relay_url, sub_id, tick_effects)
        }; // lock released

        // ── Phase 2: kernel registration (no lock held) ───────────────────
        if needs_register {
            if let (Some(url), Some(sid)) = (relay_url, sub_id) {
                kernel.register_persistent_sub(url, sid);
            }
        }

        // ── Phase 3: translate timeout / tick effects ─────────────────────
        self.translate_effects(effects, kernel)
    }
}

impl Nip46Interceptor {
    /// Translate a batch of [`Effect`]s into outbound messages and actor commands.
    ///
    /// `Subscribe` → persistent-sub registration + outbound REQ frame.
    /// `SendFrame` → outbound EVENT frame.
    /// `Progress` → `bunker_handshake_progress` via sender.
    /// `SignerReady` → `add_signer(RemoteHandle)` via sender.
    /// `DeliverResponse` → `deliver_signer_response` via sender.
    /// `Error` → `bunker_handshake_progress("failed")` via sender.
    fn translate_effects(&self, effects: Vec<Effect>, kernel: &mut Kernel) -> Vec<OutboundMessage> {
        let mut outbound = Vec::new();

        for effect in effects {
            match effect {
                Effect::Subscribe { relay_url, frame } => {
                    // Register the sub as persistent so EOSE does not auto-CLOSE
                    // the long-lived kind:24133 listener.
                    let sub_id = extract_sub_id(&frame);
                    if let Some(sid) = sub_id {
                        kernel.register_persistent_sub(relay_url.clone(), sid);
                        // Mark registration so on_idle_tick skips the duplicate call.
                        if let Ok(mut guard) = self.runtime.lock() {
                            if let Some(rt) = guard.as_mut() {
                                rt.persistent_sub_registered = true;
                            }
                        }
                    }
                    outbound.push(OutboundMessage::new(RelayRole::Signer, relay_url, frame));
                }
                Effect::SendFrame { relay_url, text } => {
                    outbound.push(OutboundMessage::new(RelayRole::Signer, relay_url, text));
                }
                Effect::Progress { stage, code, detail } => {
                    self.sender.bunker_handshake_progress(stage, code, detail);
                }
                Effect::SignerReady(ready) => {
                    self.handle_signer_ready(ready, kernel);
                }
                Effect::DeliverResponse { correlation_id: _, result } => {
                    // Steady-state RPC response: deliver to the signer's parked op.
                    self.sender.deliver_signer_response(result);
                }
                Effect::Error { error } => {
                    // Surface terminal errors as a "failed" progress event so the
                    // host spinner clears (matches signer_broker.rs:76 mapping).
                    tracing::warn!(error = %error, "nip46-runtime: handshake error");
                    self.sender.bunker_handshake_progress(
                        "failed".to_string(),
                        None,
                        Some(error.to_string()),
                    );
                    // ConnectionStateChanged preservation (V-14 / signer_broker:76).
                    self.sender.bunker_connection_state_changed(
                        "failed".to_string(),
                        Some(error.to_string()),
                    );
                }
            }
        }

        outbound
    }

    /// Build the actor-lane transport and register the resulting signer.
    fn handle_signer_ready(&self, ready: SignerReady, kernel: &mut Kernel) {
        // Resolve the remote pubkey from the hex string in SignerReady.
        let Ok(remote_pubkey) = PublicKey::from_hex(&ready.remote_signer_pubkey_hex) else {
            tracing::warn!(
                signer_pubkey = %ready.remote_signer_pubkey_hex,
                "nip46-runtime: SignerReady remote_signer_pubkey_hex unparseable"
            );
            self.sender.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some("invalid remote signer pubkey in SignerReady".to_string()),
            );
            return;
        };

        // Capture the session transport params from the runtime handle.
        let (relay_url, local_keys) = {
            let Ok(guard) = self.runtime.lock() else { return };
            let Some(rt) = guard.as_ref() else { return };
            (rt.relay_url.clone(), rt.local_keys.clone())
        };

        // Build the actor-lane transport; wrap it in an ActorLaneSignerHandle.
        let transport = ActorLaneTransport::new(
            self.sender.clone(),
            local_keys,
            remote_pubkey,
            relay_url,
        );
        let signer_source = build_remote_signer_source(
            ready.user_pubkey_hex.clone(),
            remote_pubkey,
            transport,
        );

        // Report progress before handing off the signer.
        self.sender.bunker_handshake_progress(
            "ready".to_string(),
            None,
            Some("NIP-46 signer ready".to_string()),
        );
        // ConnectionStateChanged preservation (V-14 / signer_broker:76).
        self.sender.bunker_connection_state_changed("connected".to_string(), None);

        let _ = kernel.now_secs(); // touch kernel to silence unused-mut warning
        self.sender.add_signer(signer_source, true);
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Extract the subscription id from a `["REQ", sub_id, ...]` wire frame.
fn extract_sub_id(frame: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(frame).ok()?;
    let arr = v.as_array()?;
    if arr.first()?.as_str()? != "REQ" {
        return None;
    }
    arr.get(1)?.as_str().map(str::to_string)
}

/// Build a [`SignerSource::RemoteHandle`] backed by an [`ActorLaneTransport`].
fn build_remote_signer_source(
    user_pubkey_hex: String,
    remote_pubkey: PublicKey,
    transport: ActorLaneTransport,
) -> SignerSource {
    let arc_transport: std::sync::Arc<dyn Nip46Transport> = std::sync::Arc::new(transport);
    let handle = ActorLaneSignerHandle::new(user_pubkey_hex, remote_pubkey, arc_transport);
    SignerSource::RemoteHandle(Box::new(handle))
}

// ─── ActorLaneSignerHandle ────────────────────────────────────────────────────

/// Minimal [`RemoteSignerHandle`] wrapper for the actor-lane transport.
///
/// In PR-B this will be replaced by the real `nmp-signers::Nip46Signer` which
/// parks sign operations and delivers responses through the `deliver_response`
/// hook.  For PR-A the handle must satisfy the trait so `add_signer` compiles;
/// the broker still drives the actual sign flow.
struct ActorLaneSignerHandle {
    user_pubkey_hex: String,
    remote_pubkey_hex: String,
    transport: std::sync::Arc<dyn Nip46Transport>,
}

impl ActorLaneSignerHandle {
    fn new(
        user_pubkey_hex: String,
        remote_pubkey: PublicKey,
        transport: std::sync::Arc<dyn Nip46Transport>,
    ) -> Self {
        Self {
            user_pubkey_hex,
            remote_pubkey_hex: remote_pubkey.to_hex(),
            transport,
        }
    }

    /// Build and send a NIP-46 RPC via the actor-lane transport.
    fn send_nip46_rpc(&self, method: &str, params_json: &str) -> Result<(), SignerError> {
        let rpc = Nip46Rpc {
            id: uuid_v4_hex(),
            body_json: String::new(),
            body_json_to_encrypt: format!(
                r#"{{"method":"{method}","params":[{params_json}]}}"#
            ),
            relays: Vec::new(),
            remote_pubkey_hex: self.remote_pubkey_hex.clone(),
        };
        self.transport.send_rpc(rpc)
    }
}

impl std::fmt::Debug for ActorLaneSignerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorLaneSignerHandle")
            .field("user_pubkey_hex", &self.user_pubkey_hex)
            .field("remote_pubkey_hex", &self.remote_pubkey_hex)
            .finish()
    }
}

impl RemoteSignerHandle for ActorLaneSignerHandle {
    fn pubkey_hex(&self) -> String {
        self.user_pubkey_hex.clone()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    /// Sign an unsigned event via NIP-46.
    ///
    /// PR-A: the broker still drives the actual sign flow; this path will be
    /// connected in PR-B via a parking map.  Returns an error so callers
    /// time out cleanly rather than blocking indefinitely.
    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        let unsigned_json = match serde_json::to_string(unsigned) {
            Ok(j) => j,
            Err(e) => {
                return SignerOp::err(SignerError::Backend(format!(
                    "nip46-runtime: failed to serialize unsigned event: {e}"
                )));
            }
        };
        // Fire-and-forget the RPC.  In PR-B a parking map will bridge the
        // response back through `deliver_response`.
        if let Err(e) = self.send_nip46_rpc("sign_event", &unsigned_json) {
            return SignerOp::err(e);
        }
        SignerOp::err(SignerError::Backend(
            "nip46-runtime PR-A: sign response parking not yet wired (PR-B)".to_string(),
        ))
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        let params = format!(
            r#""{}","{}""#,
            recipient_pubkey.replace('"', "\\\""),
            plaintext.replace('"', "\\\"")
        );
        if let Err(e) = self.send_nip46_rpc("nip44_encrypt", &params) {
            return SignerOp::err(e);
        }
        SignerOp::err(SignerError::Backend(
            "nip46-runtime PR-A: nip44_encrypt response parking not yet wired (PR-B)".to_string(),
        ))
    }

    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String> {
        let params = format!(
            r#""{}","{}""#,
            sender_pubkey.replace('"', "\\\""),
            ciphertext.replace('"', "\\\"")
        );
        if let Err(e) = self.send_nip46_rpc("nip44_decrypt", &params) {
            return SignerOp::err(e);
        }
        SignerOp::err(SignerError::Backend(
            "nip46-runtime PR-A: nip44_decrypt response parking not yet wired (PR-B)".to_string(),
        ))
    }

    /// Deliver a response from the relay to any parked sign operation.
    ///
    /// PR-A: responses are already delivered through the interceptor's
    /// `DeliverResponse` path via `sender.deliver_signer_response`.  This
    /// method is a no-op in PR-A; PR-B will route through the parking map.
    fn deliver_response(&self, _response_json: &str) {
        // No-op in PR-A; interceptor routes `Effect::DeliverResponse` directly
        // through `CommandSender::deliver_signer_response`.
    }
}

// ─── tiny UUID v4 substitute ─────────────────────────────────────────────────

/// Generate a hex ID for RPC correlation using timestamp nanoseconds.
///
/// Not a full UUID — just enough to distinguish sequential RPCs within a
/// single session (the session reducer guarantees at most one in-flight RPC
/// at a time in the handshake phase).
fn uuid_v4_hex() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:08x}{seq:016x}")
}
