//! NIP-55 external-signer native runtime adapter (ADR-0048 Stage 2).
//!
//! The app/core adapter for the `external_signer` capability namespace —
//! the NIP-55 analogue of [`crate::signer_broker`] (ADR-0031's
//! worker-feeds-actor indirection, with the relay transport swapped for the
//! host capability bridge):
//!
//! ```text
//! signin_nip55 ──→ Nip55Driver ──→ CapabilitySignerTransport
//!                                            │ CapabilityRequest{external_signer}
//!                                            ▼
//!                                  registered capability callback
//!                                  (Android: JNI trampoline → Kotlin
//!                                   ExternalSignerCapabilityBridge → Amber)
//!                                            │ raw ExternalSignerResponse
//!                                            ▼
//! deliver_external_signer_response ─→ Nip55Driver::deliver
//!         ├─ first-connect reply → Nip55Connect::complete → AddSigner
//!         └─ op reply           → Nip55Signer::deliver_external_response
//! ```
//!
//! Doctrine:
//! * **D7** — the host fires what Rust built and reports raw results. This
//!   module builds every request (via `nmp-signers`) and owns all policy.
//! * **D0** — `nmp-core` sees only `Box<dyn RemoteSignerHandle>` and the
//!   opaque restore hook; it never imports `nmp-signers`.
//! * **D6** — malformed responses degrade to timeout; missing capability
//!   handlers surface as `signer_state: unavailable`, never a panic.

use std::sync::{Arc, Mutex};

use nmp_core::__ffi_internal::{dispatch_capability, CapabilityCallbackSlot};
use nmp_core::substrate::{CapabilityEnvelope, CapabilityRequest};
use nmp_core::ExternalSignerHookRequest;
use nmp_signer_iface::{
    ExternalSignerRequest, ExternalSignerResponse, ExternalSignerTransport, SignerError, SignerOp,
    EXTERNAL_SIGNER_NAMESPACE,
};
use nmp_signer_iface::{RemoteSignerHandle, SignedEvent, UnsignedEvent};
use nmp_signers::{Nip55Connect, Nip55Signer, SignerPayload};

use super::NmpApp;

/// Outbound transport: serialises an [`ExternalSignerRequest`] into a
/// `CapabilityRequest { namespace: "external_signer" }` and routes it through
/// the app's registered capability callback (the existing socket — ADR-0048
/// D2: "no new FFI primitive").
///
/// The host callback is expected to *accept* the dispatch synchronously
/// (enqueue the Intent / resolver work) and reply with the actual
/// [`ExternalSignerResponse`] later via
/// [`NmpApp::deliver_external_signer_response`]. A returned error envelope
/// (missing handler, malformed request) maps to [`SignerError::Unavailable`].
pub(crate) struct CapabilitySignerTransport {
    callback: CapabilityCallbackSlot,
}

impl std::fmt::Debug for CapabilitySignerTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The gate intentionally hides the raw callback pointer/context; print
        // only registration presence.
        let registered = self.callback.is_registered();
        f.debug_struct("CapabilitySignerTransport")
            .field("callback_registered", &registered)
            .finish()
    }
}

impl ExternalSignerTransport for CapabilitySignerTransport {
    fn send_request(&self, request: ExternalSignerRequest) -> Result<(), SignerError> {
        let payload_json = serde_json::to_string(&request)
            .map_err(|e| SignerError::Backend(format!("nip55 serialize request: {e}")))?;
        let capability_request = CapabilityRequest {
            namespace: EXTERNAL_SIGNER_NAMESPACE.to_string(),
            correlation_id: request.correlation_id.clone(),
            payload_json,
        };
        let request_json = serde_json::to_string(&capability_request)
            .map_err(|e| SignerError::Backend(format!("nip55 serialize capability: {e}")))?;
        let envelope_json = dispatch_capability(&self.callback, &request_json);
        let envelope: CapabilityEnvelope = serde_json::from_str(&envelope_json)
            .map_err(|e| SignerError::Backend(format!("nip55 malformed ack envelope: {e}")))?;
        let ack: serde_json::Value =
            serde_json::from_str(&envelope.result_json).unwrap_or_default();
        match ack.get("status").and_then(|s| s.as_str()) {
            Some("dispatched") => Ok(()),
            other => Err(SignerError::Unavailable(format!(
                "external_signer capability not dispatched: {}",
                other
                    .map(str::to_string)
                    .or_else(|| ack
                        .get("reason")
                        .and_then(|r| r.as_str())
                        .map(str::to_string))
                    .unwrap_or_else(|| envelope.result_json.clone()),
            ))),
        }
    }
}

/// The NIP-55 driver: owns the in-flight first-connect, the live signer
/// registry, and the actor re-entry channel.
#[derive(Debug)]
pub(crate) struct Nip55Driver {
    tx: nmp_core::CommandSender,
    transport: Arc<CapabilitySignerTransport>,
    pending_connect: Mutex<Option<Nip55Connect>>,
    /// Live signers (shared with the actor via [`ArcNip55Signer`]). Responses
    /// that do not answer the pending connect fan out here; correlation-id
    /// routing inside `Nip55Signer::deliver_external_response` dedupes.
    signers: Mutex<Vec<Arc<Nip55Signer>>>,
}

impl Nip55Driver {
    pub(crate) fn new(
        tx: nmp_core::CommandSender,
        transport: Arc<CapabilitySignerTransport>,
    ) -> Self {
        Self {
            tx,
            transport,
            pending_connect: Mutex::new(None),
            signers: Mutex::new(Vec::new()),
        }
    }

    fn set_signer_state(&self, state: &str, reason: Option<String>) {
        self.tx
            .nip55_signer_state_changed(state.to_string(), reason);
    }

    /// Begin the first-connect `get_public_key` round-trip (ADR-0048 D2).
    pub(crate) fn signin(&self, signer_package: Option<String>) {
        let connect = Nip55Connect::new(signer_package);
        let request = connect.request().clone();
        if let Ok(mut guard) = self.pending_connect.lock() {
            if guard.is_some() {
                self.set_signer_state(
                    "awaiting_approval",
                    Some("external signer approval already pending".to_string()),
                );
                return;
            }
            *guard = Some(connect);
        } else {
            self.set_signer_state(
                "failed",
                Some("external signer pending state poisoned".to_string()),
            );
            return;
        }
        self.set_signer_state("awaiting_approval", None);
        if let Err(e) = self.transport.send_request(request) {
            if let Ok(mut guard) = self.pending_connect.lock() {
                guard.take();
            }
            self.set_signer_state("unavailable", Some(e.to_string()));
        }
    }

    /// Route a raw host response (D7: reported verbatim by the host).
    pub(crate) fn deliver(&self, response_json: &str) {
        let Ok(response) = serde_json::from_str::<ExternalSignerResponse>(response_json) else {
            return; // D6: malformed — degrade to timeout
        };

        let pending = match self.pending_connect.lock() {
            Ok(mut guard) => {
                if guard.as_ref().is_some_and(|c| c.matches(&response)) {
                    guard.take()
                } else {
                    None
                }
            }
            Err(_) => None,
        };

        if let Some(connect) = pending {
            self.complete_connect(connect, &response);
            return;
        }

        // Op reply for a live signer (ADR-0050 §D3b). Instead of fanning out to
        // the signer handles on THIS (bridge) thread, send a
        // `DeliverSignerResponse` command so the fan-out runs on the actor
        // thread (D4 single-writer) and the parked op resolves the same loop
        // iteration the inbox wakes on (no ≤250ms tick dependence, §D3a). The
        // dispatch arm fans to the remote handles via
        // `deliver_external_response`; unknown correlation ids are dropped (D6).
        // The connect/handshake path above is unchanged — it still completes on
        // the bridge thread via the `AddSigner { RemoteHandle }` re-entry.
        self.tx.deliver_signer_response(response_json.to_string());
    }

    fn complete_connect(&self, connect: Nip55Connect, response: &ExternalSignerResponse) {
        let transport: Arc<dyn ExternalSignerTransport> =
            Arc::clone(&self.transport) as Arc<dyn ExternalSignerTransport>;
        match connect.complete(response, transport) {
            Ok(signer) => {
                let signer = Arc::new(signer);
                if let Ok(mut signers) = self.signers.lock() {
                    signers.push(Arc::clone(&signer));
                }
                self.tx.add_signer(
                    nmp_core::SignerSource::RemoteHandle(Box::new(ArcNip55Signer(signer))),
                    true,
                );
                self.set_signer_state("ready", None);
            }
            Err(SignerError::Rejected(reason)) => {
                self.set_signer_state("failed", Some(reason));
            }
            Err(SignerError::Unavailable(reason)) => {
                self.set_signer_state("unavailable", Some(reason));
            }
            Err(e) => {
                self.set_signer_state("failed", Some(e.to_string()));
            }
        }
    }

    /// Reconstruct a persisted NIP-55 signer (ADR-0048 D4 — pubkey-only
    /// payload; no user interaction). Invoked by the nmp-core restore hook.
    pub(crate) fn restore(&self, payload_json: &str) {
        let payload = match serde_json::from_str::<SignerPayload>(payload_json) {
            Ok(SignerPayload::Nip55(p)) => p,
            Ok(_) => {
                self.set_signer_state(
                    "failed",
                    Some("stored signer payload is not nip55".to_string()),
                );
                return;
            }
            Err(e) => {
                self.set_signer_state("failed", Some(format!("parse signer payload: {e}")));
                return;
            }
        };
        let transport: Arc<dyn ExternalSignerTransport> =
            Arc::clone(&self.transport) as Arc<dyn ExternalSignerTransport>;
        match Nip55Signer::from_payload(&payload, transport) {
            Ok(signer) => {
                let signer = Arc::new(signer);
                if let Ok(mut signers) = self.signers.lock() {
                    signers.push(Arc::clone(&signer));
                }
                self.tx.add_signer(
                    nmp_core::SignerSource::RemoteHandle(Box::new(ArcNip55Signer(signer))),
                    true,
                );
                self.set_signer_state("ready", None);
            }
            Err(e) => {
                self.set_signer_state("failed", Some(e.to_string()));
            }
        }
    }
}

/// Adapter: `Box<dyn RemoteSignerHandle>` from an `Arc<Nip55Signer>` (the
/// `ArcRemoteSigner` precedent in `signer_broker.rs`). The driver keeps its
/// own `Arc` for response fan-out; the actor owns this boxed handle.
#[derive(Debug)]
struct ArcNip55Signer(Arc<Nip55Signer>);

impl RemoteSignerHandle for ArcNip55Signer {
    fn pubkey_hex(&self) -> String {
        RemoteSignerHandle::pubkey_hex(&*self.0)
    }

    fn signer_kind(&self) -> &'static str {
        RemoteSignerHandle::signer_kind(&*self.0)
    }

    fn persistence_payload_json(&self) -> Option<String> {
        RemoteSignerHandle::persistence_payload_json(&*self.0)
    }

    /// 90s Intent-round-trip budget (ADR-0048 D3 / ADR-0050 D4) — MUST delegate
    /// so the parked op carries the NIP-55 deadline, not the 5s NIP-46 default.
    fn op_timeout(&self) -> std::time::Duration {
        RemoteSignerHandle::op_timeout(&*self.0)
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        RemoteSignerHandle::sign(&*self.0, unsigned)
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        RemoteSignerHandle::nip44_encrypt(&*self.0, recipient_pubkey, plaintext)
    }

    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String> {
        RemoteSignerHandle::nip44_decrypt(&*self.0, sender_pubkey, ciphertext)
    }

    fn deliver_response(&self, response_json: &str) {
        RemoteSignerHandle::deliver_response(&*self.0, response_json);
    }

    fn disconnect(&self) {
        RemoteSignerHandle::disconnect(&*self.0);
    }
}

/// Initialise the NIP-55 driver for `app` and install the per-app restore hook.
/// Idempotent per app: repeated calls keep the existing per-app driver.
/// ADR-0052 §D3 — the driver handle and the restore hook are **per-app** (no
/// `GLOBAL_DRIVER` / `register_external_signer_hook` process-global), so two
/// apps have independent drivers and a freed-then-recreated app re-initialises
/// cleanly.
///
/// Runtime construction calls this before `start_runtime` can spawn the actor, fixing the
/// restore-order bug where a persisted NIP-55 account degraded because the hook
/// was installed only after host-specific Android setup. Android JNI shims may
/// still call the public symbol after registering their capability trampoline;
/// the same per-app driver is reused and reads the shared callback slot at
/// dispatch time.
pub(crate) fn init_external_signer_driver(app: &NmpApp) {
    let tx = app.actor_sender();
    let callback = Arc::clone(&app.capability_callback);
    let driver = app.external_signer_driver_get_or_init(|| {
        let transport = Arc::new(CapabilitySignerTransport { callback });
        Arc::new(Nip55Driver::new(tx, transport))
    });
    // ADR-0052 §D3 — install the restore hook into THIS app's per-app slot
    // (the actor's `IdentityRuntime` reads the matching `Arc` clone). The
    // driver's actor re-entry uses the per-app `tx` captured above — responses
    // route to the originating app structurally, no correlation token.
    let driver_for_hook = Arc::clone(&driver);
    app.install_external_signer_hook(Arc::new(move |request| match request {
        ExternalSignerHookRequest::Restore { payload_json } => {
            driver_for_hook.restore(&payload_json);
        }
    }));
}

impl NmpApp {
    /// Explicitly initialise the NIP-55 runtime. Runtime construction already
    /// installs it; this method is idempotent for hosts that want an explicit
    /// capability-registration step.
    pub fn init_external_signer(&self) {
        init_external_signer_driver(self);
    }

    /// Begin a NIP-55 sign-in (`get_public_key` + permission batch) routed to
    /// the signer app named by `signer_package` (or the OS resolver when
    /// `None`).
    pub fn signin_nip55(&self, signer_package: Option<String>) {
        self.init_external_signer();
        if let Some(driver) = self.external_signer_driver() {
            driver.signin(signer_package);
        }
    }

    /// Deliver a raw `ExternalSignerResponse` JSON reported by the host
    /// capability bridge (D7: verbatim). Routes to the pending first-connect or
    /// fans out to live signers by correlation id.
    pub fn deliver_external_signer_response(&self, response_json: &str) {
        if let Some(driver) = self.external_signer_driver() {
            driver.deliver(response_json);
        }
    }
}
