//! `Nip46Interceptor` — [`RelayTextInterceptor`] implementation.
//!
//! The interceptor is the actor's inbound gateway for the bunker relay lane.
//! On every text frame from any bunker relay it:
//!
//! 1. Filters by relay URL list (ignores frames from non-bunker relays — D6).
//! 2. Drives [`SessionState::on_relay_text`] with the frame and `now_secs`.
//! 3. Translates each returned [`Effect`] into either an outbound message or
//!    an actor command posted via the captured [`CommandSender`]:
//!    - [`Effect::Subscribe`] → register persistent sub + return outbound frame.
//!    - [`Effect::SendFrame`] → return outbound frame (signer RPC).
//!    - [`Effect::Progress`] → `CommandSender::bunker_handshake_progress`.
//!    - [`Effect::SignerReady`] → build real `Nip46Signer` + `add_signer(RemoteHandle)`.
//!    - [`Effect::DeliverResponse`] → `CommandSender::deliver_signer_response`.
//!    - [`Effect::Error`] → `CommandSender::bunker_handshake_progress("failed")`.
//! 4. When the reducer returns empty AND a decoded body is available
//!    (steady-state Done-phase path), delivers the body via
//!    `CommandSender::deliver_signer_response`.  This routes the NIP-46 RPC
//!    response to the registered `Nip46Signer`'s parking map.
//!
//! The `on_idle_tick` hook:
//! - Registers the persistent sub with the kernel on the first tick after
//!   session init (when `persistent_sub_registered == false`).
//! - Drives [`SessionState::tick`] for the 60 s per-step deadline.

use std::sync::Arc;

use nmp_core::substrate::RelayTextInterceptor;
use nmp_core::{CommandSender, Kernel, OutboundMessage, SignerSource};
use nmp_network::role::RelayRole;
use nmp_nip46::{Effect, SignerReady};
use nmp_signers::{Nip46SignerHandle, Nip46Signer};
use nmp_signer_iface::RemoteSignerHandle;
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

        // ── Phase 1: drive reducer + steady-state decode under lock ──────
        let (effects, decoded) = {
            let Ok(mut guard) = self.runtime.lock() else {
                return Vec::new();
            };
            let Some(rt) = guard.as_mut() else {
                return Vec::new();
            };
            rt.on_relay_text(relay_url, text, now)
        }; // lock released

        // ── Phase 2: deliver steady-state RPC response (no lock) ─────────
        // When effects is empty and a decoded body is available, the session
        // is in Done phase and this is a kind:24133 response from the bunker.
        // Deliver it to the registered Nip46Signer via deliver_signer_response
        // so the parking map resolves the pending sign operation.
        if let Some(body) = decoded {
            self.sender.deliver_signer_response(body);
        }

        // ── Phase 3: translate handshake effects (kernel + sender, no lock)
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
            let relay_url =
                if needs_reg { Some(rt.relay_urls.first().cloned().unwrap_or_default()) } else { None };
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
    /// `SignerReady` → build real `Nip46Signer` + `add_signer(RemoteHandle)`.
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

    /// Build the real `Nip46Signer` and register it as the account signer.
    ///
    /// Called on [`Effect::SignerReady`] (handshake complete). Builds:
    /// - [`ActorLaneTransport`] (multi-relay, fans outbound RPCs to all relays).
    /// - [`Nip46Signer`] (real parking map — no longer a placeholder).
    /// - [`ArcRemoteSigner`] wrapper for `Box<dyn RemoteSignerHandle>`.
    ///
    /// The `Nip46Signer` receives subsequent inbound responses via the actor's
    /// `DeliverSignerResponse` path (§D3b fan-out) and resolves parked sign
    /// operations via `ingest_rpc_response`.
    fn handle_signer_ready(&self, ready: SignerReady, kernel: &mut Kernel) {
        // Resolve the remote signer pubkey from the SignerReady hex string.
        let Ok(remote_signer_pubkey) = PublicKey::from_hex(&ready.remote_signer_pubkey_hex) else {
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

        // Resolve the user pubkey (the account's actual public key).
        let Ok(user_pubkey) = PublicKey::from_hex(&ready.user_pubkey_hex) else {
            tracing::warn!(
                user_pubkey = %ready.user_pubkey_hex,
                "nip46-runtime: SignerReady user_pubkey_hex unparseable"
            );
            self.sender.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some("invalid user pubkey in SignerReady".to_string()),
            );
            return;
        };

        // Capture session transport params from the runtime handle.
        let (relay_urls, local_keys) = {
            let Ok(guard) = self.runtime.lock() else { return };
            let Some(rt) = guard.as_ref() else { return };
            (rt.relay_urls.clone(), rt.local_keys.clone())
        };

        // Build the multi-relay transport (fans each RPC to ALL relay URLs).
        let transport = ActorLaneTransport::new_multi(
            self.sender.clone(),
            local_keys.clone(),
            remote_signer_pubkey,
            relay_urls.clone(),
        );

        // Build a synthetic bunker:// URI so Nip46SignerHandle can parse the
        // remote pubkey and relay list.  Uses the session's local secret key so
        // the signer encrypts with the same ephemeral key used during the
        // handshake.
        let relay_params: String = relay_urls
            .iter()
            .map(|u| format!("&relay={}", nmp_nip46::percent_encode_query_value(u)))
            .collect();
        let synthetic_uri = format!(
            "bunker://{}?{}",
            remote_signer_pubkey.to_hex(),
            relay_params.trim_start_matches('&'),
        );

        let local_sk = local_keys.secret_key().clone();
        let signer_handle = match Nip46SignerHandle::from_bunker_uri_with_local_key(
            &synthetic_uri,
            local_sk,
        ) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(error = %e, "nip46-runtime: failed to build Nip46SignerHandle");
                self.sender.bunker_handshake_progress(
                    "failed".to_string(),
                    None,
                    Some(format!("internal: signer handle build failed: {e}")),
                );
                return;
            }
        };

        // Complete the signer (handshake already done — supply user pubkey directly).
        let signer: Nip46Signer = signer_handle.complete(Arc::new(transport), user_pubkey);

        // Wrap in ArcRemoteSigner so we can clone the Arc for dual-use later
        // (PR-B2 will clean this up). The `Box<dyn RemoteSignerHandle>` is
        // handed to the kernel; `Nip46Signer::deliver_response` routes inbound
        // responses through `ingest_rpc_response` to resolve parked sign ops.
        let signer_source = SignerSource::RemoteHandle(Box::new(ArcRemoteSigner(Arc::new(signer))));

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

// ─── ArcRemoteSigner ─────────────────────────────────────────────────────────

/// Local `Arc<Nip46Signer>` wrapper for `Box<dyn RemoteSignerHandle>`.
///
/// `nmp-ffi/src/signer_broker.rs` has its own copy of this wrapper for the
/// broker path (do NOT touch that one — broker still drives native NIP-46 until
/// PR-B2). This copy lives in the runtime interceptor for the actor-lane path.
/// PR-B2 will unify them when the broker is deleted.
///
/// The `Arc` is needed so the sign call (which parks in `Nip46Signer::pending`)
/// and the response delivery (which calls `ingest_rpc_response`) both refer to
/// the SAME `Nip46Signer` instance.  `RemoteSignerHandle::sign` and
/// `deliver_response` both go through the same `Arc` clone.
struct ArcRemoteSigner(Arc<Nip46Signer>);

impl std::fmt::Debug for ArcRemoteSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArcRemoteSigner")
            .field("signer", &self.0)
            .finish()
    }
}

impl RemoteSignerHandle for ArcRemoteSigner {
    fn pubkey_hex(&self) -> String {
        self.0.pubkey_hex()
    }

    fn signer_kind(&self) -> &'static str {
        self.0.signer_kind()
    }

    fn persistence_payload_json(&self) -> Option<String> {
        self.0.persistence_payload_json()
    }

    fn sign(&self, unsigned: &nmp_signer_iface::signing::UnsignedEvent) -> nmp_signer_iface::SignerOp<nmp_signer_iface::signing::SignedEvent> {
        self.0.sign(unsigned)
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> nmp_signer_iface::SignerOp<String> {
        self.0.nip44_encrypt(recipient_pubkey, plaintext)
    }

    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> nmp_signer_iface::SignerOp<String> {
        self.0.nip44_decrypt(sender_pubkey, ciphertext)
    }

    fn nip44_decrypt_session_begin(
        &self,
        request: nmp_signer_iface::Nip44DecryptSessionBeginRequest,
    ) -> nmp_signer_iface::SignerOp<nmp_signer_iface::Nip44DecryptSessionGrant> {
        self.0.nip44_decrypt_session_begin(request)
    }

    fn nip44_decrypt_batch(
        &self,
        request: nmp_signer_iface::Nip44DecryptBatchRequest,
    ) -> nmp_signer_iface::SignerOp<nmp_signer_iface::Nip44DecryptBatchResult> {
        self.0.nip44_decrypt_batch(request)
    }

    fn nip44_decrypt_session_end(
        &self,
        request: nmp_signer_iface::Nip44DecryptSessionEndRequest,
    ) -> nmp_signer_iface::SignerOp<bool> {
        self.0.nip44_decrypt_session_end(request)
    }

    fn deliver_response(&self, response_json: &str) {
        self.0.deliver_response(response_json);
    }

    fn disconnect(&self) {
        self.0.disconnect();
    }
}
