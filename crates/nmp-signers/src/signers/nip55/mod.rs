//! NIP-55 external-signer (`nostrsigner:` / Amber on Android).
//!
//! ## Architecture
//!
//! `Nip55Signer` is the third `RemoteSignerHandle` implementor alongside
//! `Nip46Signer`.  It holds the user's pubkey (learned from the first
//! `get_public_key` call), a `signer_package` string once the host reports
//! which Amber package responded, a per-connection `granted_permissions` list
//! for the `ContentResolver` fast-path, and a `pending` correlation table.
//!
//! Unlike NIP-46 (relay round-trip, < 1s), an Android Intent round-trip
//! requires the user to foreground Amber (5–30s). The per-op deadline is
//! therefore `EXTERNAL_SIGN_TIMEOUT` = 90s (ADR-0048 D3), reported via
//! `RemoteSignerHandle::op_timeout()`.
//!
//! ## Key-material boundary
//!
//! The user's private key NEVER enters this crate. `Nip55Signer` only knows
//! the pubkey (hex), the signer package name, and a permission batch. All
//! cryptographic operations are delegated to the external signer app over IPC.
//!
//! ## Persistence
//!
//! `to_payload()` serialises to `SignerPayload::Nip55` — pubkey-only. No
//! secret is persisted; restoring the signer does not require user interaction
//! beyond re-launching the app (the signer package is cached too so the host
//! can route the first `ContentResolver` fast-path without a `get_public_key`
//! probe).
//!
//! ## Host contract (D7)
//!
//! The host fires `ExternalSignerRequest`s and reports `ExternalSignerResponse`s
//! verbatim — it never retries, interprets, or decides policy. Rust controls
//! all retry logic (force_interactive re-issue on `Unavailable`).

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use nmp_signer_iface::{
    ExternalSignerMethod, ExternalSignerOutcome, ExternalSignerRequest, ExternalSignerResponse,
    ExternalSignerTransport, Nip55Permission, SignerError, SignerOp,
};
use nostr::PublicKey;
use serde_json;

use crate::signers::payload::{Nip55Payload, SignerPayload};
use crate::signers::traits::{Nip44, Signer, SignerBackend};

mod connect;
mod handle;
pub(crate) mod mapper;

pub use connect::Nip55Connect;

/// Pending request correlation table: correlation_id → parked request state.
type PendingMap = HashMap<String, PendingRequest>;

struct PendingRequest {
    request: ExternalSignerRequest,
    sender: Sender<Result<String, SignerError>>,
}

/// Fully-initialised NIP-55 signer.
///
/// Created either by `Nip55Signer::new` (first connect, after `get_public_key`
/// round-trip) or `Nip55Signer::from_payload` (restore from persisted pubkey).
///
/// All mutable state (pending ops, signer_package, granted_permissions) is
/// guarded by an `Arc<Mutex<…>>` so the signer can be shared across thread
/// boundaries without copying — the actor holds `Box<dyn RemoteSignerHandle>`,
/// which is `Send + Sync`.
pub struct Nip55Signer {
    /// The user's pubkey. Learned from `get_public_key` and cached here.
    user_pubkey: PublicKey,
    /// Guarded mutable session state.
    state: Arc<Mutex<Nip55State>>,
    /// Transport to the host capability bridge (injected; production = FFI
    /// capability socket; test = `FakeExternalSignerTransport`).
    transport: Arc<dyn ExternalSignerTransport>,
}

struct Nip55State {
    /// Package name of the signer app as reported by the OS on the first
    /// successful `get_public_key` reply. `None` before the first successful
    /// connect (unusual — typically set at construction).
    signer_package: Option<String>,
    /// Permissions granted in the first-connect batch. Non-empty after the
    /// initial `get_public_key` request that carries `permissions: [...]`.
    granted_permissions: Vec<Nip55Permission>,
    /// In-flight correlation_id → response channel.
    pending: PendingMap,
    /// Correlation id currently occupying Android's single interactive Intent
    /// result slot. ContentResolver requests may run concurrently; Intent
    /// approvals are serialized by policy before native dispatch.
    interactive_in_flight: Option<String>,
}

/// Generate a unique correlation id (16-hex chars).
///
/// Uses an atomic counter + timestamp nanoseconds so successive calls within
/// the same nanosecond are still unique. Not cryptographic — uniqueness within
/// the pending map lifetime is all that's required.
fn generate_correlation_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH}; // doctrine-allow: D20 — NIP-55 is the native Android external signer (nostrsigner: Intent); it is permanently excluded from any wasm32 target and will never become wasm-reachable
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mixed = now.wrapping_add(n.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    format!("{mixed:016x}{n:08x}")
}

impl Nip55Signer {
    /// Construct a fully-initialised signer from a known pubkey.
    ///
    /// `signer_package` should be `Some` when known (e.g. restored from
    /// payload); `None` is acceptable on a brand-new connect where the host
    /// is still discovering which Amber package to route to.
    pub fn new(
        user_pubkey: PublicKey,
        signer_package: Option<String>,
        granted_permissions: Vec<Nip55Permission>,
        transport: Arc<dyn ExternalSignerTransport>,
    ) -> Self {
        Self {
            user_pubkey,
            state: Arc::new(Mutex::new(Nip55State {
                signer_package,
                granted_permissions,
                pending: HashMap::new(),
                interactive_in_flight: None,
            })),
            transport,
        }
    }

    /// Restore from a persisted payload. No user interaction required; the
    /// signer is immediately usable for new sign requests.
    ///
    /// `granted_permissions` is taken verbatim from the persisted payload —
    /// an empty persisted batch restores with an empty grant set rather than
    /// synthesizing a framework default (§9: the permission batch is an
    /// app-owned policy fact, not something `nmp-signers` may invent). A
    /// signer restored with no grants self-heals on first use: the live
    /// request carries no `granted_permissions`, so the host falls back to an
    /// interactive Intent and the app's first-connect re-grant repopulates
    /// the batch.
    pub fn from_payload(
        p: &Nip55Payload,
        transport: Arc<dyn ExternalSignerTransport>,
    ) -> Result<Self, SignerError> {
        let user_pubkey = PublicKey::from_hex(&p.user_pubkey_hex)
            .map_err(|e| SignerError::Backend(format!("invalid nip55 cached pubkey: {e}")))?;
        let granted_permissions = p
            .granted_permissions
            .iter()
            .map(|s| Nip55Permission { kind: s.clone() })
            .collect();
        Ok(Self::new(
            user_pubkey,
            p.signer_package.clone(),
            granted_permissions,
            transport,
        ))
    }

    /// Deliver a host response by correlation id.
    ///
    /// Deserialises `response_json` as `ExternalSignerResponse`, looks up the
    /// pending request by `correlation_id`, and either resolves it or retries a
    /// revoked ContentResolver fast-path once via a forced interactive Intent.
    /// Unknown correlation ids are silently dropped (D6 — bad frames degrade
    /// to timeout).
    pub fn deliver_external_response(&self, response_json: &str) {
        let resp: ExternalSignerResponse = match serde_json::from_str(response_json) {
            Ok(r) => r,
            Err(_) => return, // malformed — degrade to timeout
        };

        let correlation_id = resp.correlation_id.clone();
        let mut retry_request = None;
        let mut completed = None;

        if let Ok(mut state) = self.state.lock() {
            // Update the cached signer_package on a successful get_public_key
            // reply (the host reports it in the response).
            if let ExternalSignerOutcome::Ok { .. } = &resp.outcome {
                if let Some(pkg) = resp.signer_package.clone() {
                    state.signer_package = Some(pkg);
                }
            }

            let Some(mut pending) = state.pending.remove(&correlation_id) else {
                return;
            };
            if state.interactive_in_flight.as_deref() == Some(correlation_id.as_str()) {
                state.interactive_in_flight = None;
            }

            match resp.outcome {
                ExternalSignerOutcome::Ok { result } => {
                    completed = Some((pending.sender, Ok(result)));
                }
                ExternalSignerOutcome::Rejected { reason } => {
                    completed = Some((pending.sender, Err(SignerError::Rejected(reason))));
                }
                ExternalSignerOutcome::SignerError { reason } => {
                    completed = Some((pending.sender, Err(SignerError::Backend(reason))));
                }
                ExternalSignerOutcome::Unavailable { reason } => {
                    if pending.request.uses_content_resolver_fast_path() {
                        if state.interactive_in_flight.is_none() {
                            pending.request.force_interactive = true;
                            state.interactive_in_flight = Some(correlation_id.clone());
                            retry_request = Some(pending.request.clone());
                            state.pending.insert(correlation_id.clone(), pending);
                        } else {
                            completed = Some((
                                pending.sender,
                                Err(SignerError::Unavailable(format!(
                                    "external signer approval already pending; resolver retry blocked: {reason}"
                                ))),
                            ));
                        }
                    } else {
                        completed = Some((pending.sender, Err(SignerError::Unavailable(reason))));
                    }
                }
            }
        }

        if let Some(request) = retry_request {
            if let Err(e) = self.transport.send_request(request) {
                if let Ok(mut state) = self.state.lock() {
                    if state.interactive_in_flight.as_deref() == Some(correlation_id.as_str()) {
                        state.interactive_in_flight = None;
                    }
                    if let Some(pending) = state.pending.remove(&correlation_id) {
                        completed = Some((pending.sender, Err(e)));
                    }
                }
            }
        }

        if let Some((sender, result)) = completed {
            let _ = sender.send(result);
        }
    }

    fn drain_pending_with_error_locked(state: &mut Nip55State, msg: &str) {
        state.interactive_in_flight = None;
        for (_id, pending) in state.pending.drain() {
            let _ = pending
                .sender
                .send(Err(SignerError::Rejected(msg.to_string())));
        }
    }

    fn clear_pending_after_send_error(
        &self,
        correlation_id: &str,
        err: SignerError,
    ) -> SignerOp<String> {
        if let Ok(mut state) = self.state.lock() {
            if state.interactive_in_flight.as_deref() == Some(correlation_id) {
                state.interactive_in_flight = None;
            }
            state.pending.remove(correlation_id);
        }
        SignerOp::err(err)
    }

    /// Drain every pending op with an error. Called on disconnect/account
    /// removal so blocked ops fail fast rather than waiting for the 90s
    /// deadline to elapse.
    pub fn drain_pending_with_error(&self, msg: &str) {
        if let Ok(mut state) = self.state.lock() {
            Self::drain_pending_with_error_locked(&mut state, msg);
        }
    }

    /// The user's pubkey.
    pub fn user_pubkey(&self) -> PublicKey {
        self.user_pubkey
    }

    /// Number of in-flight ops awaiting a response. Test-only.
    #[cfg(test)]
    pub(crate) fn pending_len(&self) -> usize {
        self.state.lock().map(|s| s.pending.len()).unwrap_or(0)
    }

    /// Current signer package (test helper).
    #[cfg(test)]
    pub(crate) fn signer_package(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|s| s.signer_package.clone())
    }

    /// Enqueue an outbound request and park a one-shot receiver.
    ///
    /// Builds the `ExternalSignerRequest` from the given parts, registers a
    /// `Sender` in the pending map under the generated `correlation_id`, and
    /// hands the request to the transport. Returns `SignerOp::Pending(rx)`.
    ///
    /// On transport error the pending entry is removed immediately (otherwise
    /// it leaks until the 90s deadline fires).
    fn enqueue(
        &self,
        method: ExternalSignerMethod,
        payload: String,
        counterparty: Option<String>,
        include_permissions: bool,
    ) -> SignerOp<String> {
        let correlation_id = generate_correlation_id();
        let (state_pkg, requested_permissions, granted_permissions) = {
            match self.state.lock() {
                Ok(s) => (
                    s.signer_package.clone(),
                    if include_permissions {
                        s.granted_permissions.clone()
                    } else {
                        Vec::new()
                    },
                    s.granted_permissions.clone(),
                ),
                Err(_) => return SignerOp::err(SignerError::Backend("state poisoned".to_string())),
            }
        };

        let request = ExternalSignerRequest {
            correlation_id: correlation_id.clone(),
            method,
            payload,
            current_user: Some(self.user_pubkey.to_hex()),
            counterparty,
            permissions: requested_permissions,
            granted_permissions,
            signer_package: state_pkg,
            force_interactive: false,
        };

        let (tx, rx) = mpsc::channel();

        if let Ok(mut state) = self.state.lock() {
            if request.requires_interactive_intent() {
                if state.interactive_in_flight.is_some() {
                    return SignerOp::err(SignerError::Unavailable(
                        "external signer approval already pending".to_string(),
                    ));
                }
                state.interactive_in_flight = Some(correlation_id.clone());
            }
            state.pending.insert(
                correlation_id.clone(),
                PendingRequest {
                    request: request.clone(),
                    sender: tx,
                },
            );
        } else {
            return SignerOp::err(SignerError::Backend("state poisoned".to_string()));
        }

        if let Err(e) = self.transport.send_request(request) {
            return self.clear_pending_after_send_error(&correlation_id, e);
        }

        SignerOp::Pending(rx)
    }
}

impl std::fmt::Debug for Nip55Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (pkg, pending_count) = self
            .state
            .lock()
            .map(|s| (s.signer_package.clone(), s.pending.len()))
            .unwrap_or((None, 0));
        f.debug_struct("Nip55Signer")
            .field("user_pubkey", &self.user_pubkey.to_hex())
            .field("signer_package", &pkg)
            .field("pending_count", &pending_count)
            .finish_non_exhaustive()
    }
}

impl Signer for Nip55Signer {
    fn backend(&self) -> SignerBackend {
        SignerBackend::Nip55
    }

    fn pubkey(&self) -> PublicKey {
        self.user_pubkey
    }

    fn sign(
        &self,
        unsigned: nmp_signer_iface::UnsignedEvent,
    ) -> SignerOp<nmp_signer_iface::SignedEvent> {
        let payload = match serde_json::to_string(&unsigned) {
            Ok(s) => s,
            Err(e) => {
                return SignerOp::err(SignerError::Backend(format!(
                    "nip55 serialize unsigned: {e}"
                )))
            }
        };
        let raw_op = self.enqueue(ExternalSignerMethod::SignEvent, payload, None, false);
        mapper::map_response_to_event(raw_op, unsigned, self.user_pubkey)
    }

    fn nip44(&self) -> Option<&dyn Nip44> {
        Some(self)
    }

    fn to_payload(&self) -> Result<SignerPayload, nmp_signer_iface::SignerError> {
        let state = self.state.lock();
        let (signer_package, granted_permissions) = state
            .map(|s| {
                (
                    s.signer_package.clone(),
                    s.granted_permissions
                        .iter()
                        .map(|p| p.kind.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_default();
        Ok(SignerPayload::Nip55(Nip55Payload {
            user_pubkey_hex: self.user_pubkey.to_hex(),
            signer_package,
            granted_permissions,
        }))
    }
}

impl Nip44 for Nip55Signer {
    fn encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String> {
        self.enqueue(
            ExternalSignerMethod::Nip44Encrypt,
            plaintext.to_string(),
            Some(recipient.to_hex()),
            false,
        )
    }

    fn decrypt(&self, sender: &PublicKey, ciphertext: &str) -> SignerOp<String> {
        self.enqueue(
            ExternalSignerMethod::Nip44Decrypt,
            ciphertext.to_string(),
            Some(sender.to_hex()),
            false,
        )
    }
}

#[cfg(test)]
pub(crate) mod tests;
