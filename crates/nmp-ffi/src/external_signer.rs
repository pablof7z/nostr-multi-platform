//! NIP-55 external-signer C-ABI adapter (ADR-0048 Stage 2).
//!
//! The app/core adapter for the `external_signer` capability namespace —
//! the NIP-55 analogue of [`crate::signer_broker`] (ADR-0031's
//! worker-feeds-actor indirection, with the relay transport swapped for the
//! host capability bridge):
//!
//! ```text
//! nmp_app_signin_nip55 ──→ Nip55Driver ──→ CapabilitySignerTransport
//!                                            │ CapabilityRequest{external_signer}
//!                                            ▼
//!                                  registered capability callback
//!                                  (Android: JNI trampoline → Kotlin
//!                                   ExternalSignerCapabilityBridge → Amber)
//!                                            │ raw ExternalSignerResponse
//!                                            ▼
//! nmp_app_deliver_external_signer_response ─→ Nip55Driver::deliver
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

use std::ffi::c_char;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use nmp_core::__ffi_internal::{dispatch_capability, CapabilityCallbackSlot};
use nmp_core::substrate::{CapabilityEnvelope, CapabilityRequest, SignedEvent, UnsignedEvent};
use nmp_core::{
    register_external_signer_hook, ActorCommand, ExternalSignerHookRequest, RemoteSignerHandle,
};
use nmp_signer_iface::{
    ExternalSignerRequest, ExternalSignerResponse, ExternalSignerTransport, SignerError, SignerOp,
    EXTERNAL_SIGNER_NAMESPACE,
};
use nmp_signers::{Nip55Connect, Nip55Signer, SignerPayload};

use super::{app_ref, NmpApp};

/// Process-global driver handle (mirrors `GLOBAL_BROKER` in
/// `signer_broker.rs` — one app per process; the deliver/signin symbols
/// reach the driver without a second registration mechanism).
static GLOBAL_DRIVER: OnceLock<Arc<Nip55Driver>> = OnceLock::new();

/// Outbound transport: serialises an [`ExternalSignerRequest`] into a
/// `CapabilityRequest { namespace: "external_signer" }` and routes it through
/// the app's registered capability callback (the existing socket — ADR-0048
/// D2: "no new FFI primitive").
///
/// The host callback is expected to *accept* the dispatch synchronously
/// (enqueue the Intent / resolver work) and reply with the actual
/// [`ExternalSignerResponse`] later via
/// [`nmp_app_deliver_external_signer_response`]. A returned error envelope
/// (missing handler, malformed request) maps to [`SignerError::Unavailable`].
pub(crate) struct CapabilitySignerTransport {
    callback: CapabilityCallbackSlot,
}

impl std::fmt::Debug for CapabilitySignerTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `CapabilityCallbackRegistration` is a raw fn pointer + context and
        // does not implement Debug — print only registration presence.
        let registered = self
            .callback
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false);
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
    pub(crate) fn new(tx: nmp_core::CommandSender, transport: Arc<CapabilitySignerTransport>) -> Self {
        Self {
            tx,
            transport,
            pending_connect: Mutex::new(None),
            signers: Mutex::new(Vec::new()),
        }
    }

    fn set_signer_state(&self, state: &str, reason: Option<String>) {
        let _ = self.tx.send(ActorCommand::Nip55SignerStateChanged {
            state: state.to_string(),
            reason,
        });
    }

    /// Begin the first-connect `get_public_key` round-trip (ADR-0048 D2).
    pub(crate) fn signin(&self, signer_package: Option<String>) {
        let connect = Nip55Connect::new(signer_package);
        let request = connect.request().clone();
        if let Ok(mut guard) = self.pending_connect.lock() {
            *guard = Some(connect);
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

        // Op reply for a live signer — fan out; unknown correlation ids are
        // dropped inside `deliver_external_response` (D6).
        if let Ok(signers) = self.signers.lock() {
            for signer in signers.iter() {
                signer.deliver_external_response(response_json);
            }
        }
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
                let _ = self.tx.send(ActorCommand::AddSigner {
                    source: nmp_core::SignerSource::RemoteHandle(Box::new(ArcNip55Signer(
                        signer,
                    ))),
                    make_active: true,
                });
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
                let _ = self.tx.send(ActorCommand::AddSigner {
                    source: nmp_core::SignerSource::RemoteHandle(Box::new(ArcNip55Signer(
                        signer,
                    ))),
                    make_active: true,
                });
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

    /// 90s Intent-round-trip budget (ADR-0048 D3) — MUST delegate so the
    /// parked op carries the NIP-55 deadline, not the 5s NIP-46 default.
    fn sign_timeout(&self) -> std::time::Duration {
        RemoteSignerHandle::sign_timeout(&*self.0)
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

/// Initialise the NIP-55 driver for `app` and register the nmp-core restore
/// hook. Idempotent: repeated calls after the first keep the existing
/// process-global driver (the `nmp_signer_broker_init` contract).
///
/// Called by the Android JNI shims at `nativeNew`, after the capability
/// callback trampoline is registered.
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()` and not yet
/// freed. Passing null is a safe no-op.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_external_signer_init(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let tx = app.actor_sender();
    let callback = Arc::clone(&app.capability_callback);
    let _ = GLOBAL_DRIVER.get_or_init(|| {
        let transport = Arc::new(CapabilitySignerTransport { callback });
        let driver = Arc::new(Nip55Driver::new(tx, transport));
        let driver_for_hook = Arc::clone(&driver);
        register_external_signer_hook(Arc::new(move |request| match request {
            ExternalSignerHookRequest::Restore { payload_json } => {
                driver_for_hook.restore(&payload_json);
            }
        }));
        driver
    });
}

/// Begin a NIP-55 sign-in (`get_public_key` + permission batch) routed to
/// the signer app named by `signer_package` (or the OS resolver when null).
///
/// Requires [`nmp_external_signer_init`] to have run; otherwise this is a
/// no-op (defence against init-order bugs — the host UI gates the affordance
/// on detection, so an uninitialised driver is unreachable in normal flow).
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()`. Null `app` or
/// `signer_package` are safe (`signer_package == null` means "let the OS
/// resolver pick").
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_signin_nip55(app: *mut NmpApp, signer_package: *const c_char) {
    // Lazy-init keeps the symbol safe even if a host forgets the init call.
    nmp_external_signer_init(app);
    if app_ref(app).is_none() {
        return;
    }
    let package = if signer_package.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null means a valid C string for the
        // call duration. Invalid UTF-8 degrades to no package hint.
        unsafe { std::ffi::CStr::from_ptr(signer_package).to_str() }
            .ok()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    if let Some(driver) = GLOBAL_DRIVER.get() {
        driver.signin(package);
    }
}

/// Deliver a raw `ExternalSignerResponse` JSON reported by the host
/// capability bridge (D7 — verbatim). Routes to the pending first-connect
/// or fans out to live signers by correlation id.
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()`. Null arguments
/// are safe no-ops.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_deliver_external_signer_response(
    app: *mut NmpApp,
    response_json: *const c_char,
) {
    if app_ref(app).is_none() {
        return;
    }
    let Some(response) = super::c_string_argument(response_json) else {
        return;
    };
    if let Some(driver) = GLOBAL_DRIVER.get() {
        driver.deliver(&response);
    }
}

#[cfg(test)]
mod tests {
    //! Driver loop tests — the Rust half of the ADR-0048 Stage-2 done-gate:
    //! signin builds the `get_public_key` request (with the permission
    //! batch), the host's raw reply resolves into `AddSigner { RemoteHandle
    //! (nip55), make_active }` + `signer_state: ready`, and a subsequent
    //! sign round-trips through the SAME transport and verifies end to end.

    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    use nmp_signer_iface::{ExternalSignerMethod, ExternalSignerOutcome};

    /// Build a production driver wired through the REAL
    /// `CapabilitySignerTransport` + capability socket, against a mock
    /// native handler that acks every dispatch (the role the Android JNI
    /// trampoline plays) and records the `payload_json` so tests can assert
    /// on the exact request Rust built.
    fn make_driver(tx: nmp_core::CommandSender) -> Nip55Driver {
        let slot = nmp_core::__ffi_internal::new_capability_callback_slot();
        install_dispatch_ack(&slot);
        let transport = Arc::new(CapabilitySignerTransport { callback: slot });
        Nip55Driver::new(tx, transport)
    }

    /// Handler that acks every external_signer dispatch and records the
    /// payload into a process-global so tests can assert on the request.
    /// (FFI-shaped `extern "C" fn` cannot capture state — the keyring mock
    /// in `capability.rs` uses the same pattern.)
    static DISPATCHED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    extern "C" fn ack_handler(
        _ctx: *mut std::ffi::c_void,
        request_json: *const c_char,
    ) -> *mut c_char {
        let request = unsafe { std::ffi::CStr::from_ptr(request_json) }
            .to_string_lossy()
            .into_owned();
        let parsed: serde_json::Value = serde_json::from_str(&request).unwrap_or_default();
        let namespace = parsed
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let correlation_id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(payload) = parsed.get("payload_json").and_then(|v| v.as_str()) {
            DISPATCHED.lock().unwrap().push(payload.to_string());
        }
        let envelope = CapabilityEnvelope {
            namespace,
            correlation_id,
            result_json: r#"{"status":"dispatched"}"#.to_string(),
        };
        std::ffi::CString::new(serde_json::to_string(&envelope).unwrap())
            .unwrap()
            .into_raw()
    }

    fn install_dispatch_ack(slot: &CapabilityCallbackSlot) {
        *slot.lock().unwrap() = Some(nmp_core::__ffi_internal::CapabilityCallbackRegistration {
            context: 0,
            callback: ack_handler,
        });
    }

    fn last_dispatched_request() -> ExternalSignerRequest {
        let guard = DISPATCHED.lock().unwrap();
        serde_json::from_str(guard.last().expect("a request was dispatched"))
            .expect("dispatched payload parses as ExternalSignerRequest")
    }

    /// Serialise access to the DISPATCHED global across tests.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn signin_loop_resolves_add_signer_and_sign_round_trip() {
        let _g = SERIAL.lock().unwrap();
        DISPATCHED.lock().unwrap().clear();
        let (tx, rx) = mpsc::channel();
        let driver = make_driver(tx);

        // 1. signin → awaiting_approval + a get_public_key dispatch with the
        //    permission batch (Rust decides what to ask for — D2).
        driver.signin(Some("com.greenart7c3.nostrsigner".to_string()));
        match rx.try_recv().expect("state command sent") {
            ActorCommand::Nip55SignerStateChanged { state, .. } => {
                assert_eq!(state, "awaiting_approval");
            }
            other => panic!("expected Nip55SignerStateChanged, got {other:?}"),
        }
        let connect_request = last_dispatched_request();
        assert_eq!(connect_request.method, ExternalSignerMethod::GetPublicKey);
        assert!(
            !connect_request.permissions.is_empty(),
            "first connect must carry the permission batch"
        );
        assert_eq!(
            connect_request.signer_package.as_deref(),
            Some("com.greenart7c3.nostrsigner")
        );

        // 2. Host reports the raw pubkey reply → AddSigner{nip55, active}.
        let keys = nostr::Keys::generate();
        let reply = ExternalSignerResponse {
            correlation_id: connect_request.correlation_id.clone(),
            outcome: ExternalSignerOutcome::Ok {
                result: keys.public_key().to_hex(),
            },
            signer_package: Some("com.greenart7c3.nostrsigner".to_string()),
        };
        driver.deliver(&serde_json::to_string(&reply).unwrap());

        let handle = match rx.try_recv().expect("AddSigner sent") {
            ActorCommand::AddSigner {
                source: nmp_core::SignerSource::RemoteHandle(handle),
                make_active,
            } => {
                assert!(make_active);
                handle
            }
            other => panic!("expected AddSigner, got {other:?}"),
        };
        assert_eq!(handle.signer_kind(), "nip55");
        assert_eq!(handle.pubkey_hex(), keys.public_key().to_hex());
        assert_eq!(handle.sign_timeout(), Duration::from_secs(90));
        match rx.try_recv().expect("ready state sent") {
            ActorCommand::Nip55SignerStateChanged { state, .. } => assert_eq!(state, "ready"),
            other => panic!("expected ready state, got {other:?}"),
        }

        // 3. Sign through the actor-held handle → a sign_event dispatch on
        //    the SAME transport; the raw signed-event reply resolves the
        //    parked op with full id+sig verification (the mapper).
        let unsigned = UnsignedEvent {
            pubkey: keys.public_key().to_hex(),
            kind: 1,
            tags: vec![],
            content: "loop proof".to_string(),
            created_at: 1_700_000_000,
        };
        let op = handle.sign(&unsigned);

        let sign_request = last_dispatched_request();
        assert_eq!(sign_request.method, ExternalSignerMethod::SignEvent);
        assert!(
            sign_request.permissions.is_empty(),
            "non-connect ops must not re-send the permission batch"
        );

        // Stand in for Amber: actually sign the event the request asked for
        // (the mapper recomputes the id and verifies the schnorr signature,
        // so the reply must be a REAL signed event).
        let signed_json = amber_sign(&keys, &sign_request.payload);
        let sign_reply = ExternalSignerResponse {
            correlation_id: sign_request.correlation_id.clone(),
            outcome: ExternalSignerOutcome::Ok {
                result: signed_json,
            },
            signer_package: None,
        };
        driver.deliver(&serde_json::to_string(&sign_reply).unwrap());

        let resolved = op
            .wait(Duration::from_secs(2))
            .expect("sign op resolves with the Amber-signed event");
        assert_eq!(resolved.unsigned.pubkey, keys.public_key().to_hex());
        assert_eq!(resolved.unsigned.content, "loop proof");
    }

    /// Sign the requested unsigned-event payload with `keys` and return the
    /// signed-event JSON, exactly as Amber would.
    fn amber_sign(keys: &nostr::Keys, payload: &str) -> String {
        use nostr::JsonUtil;
        let v: serde_json::Value = serde_json::from_str(payload).expect("payload is JSON");
        let kind = nostr::Kind::from_u16(u16::try_from(v["kind"].as_u64().unwrap_or(1)).unwrap());
        let event = nostr::EventBuilder::new(kind, v["content"].as_str().unwrap_or_default())
            .custom_created_at(nostr::Timestamp::from(
                v["created_at"].as_u64().unwrap_or_default(),
            ))
            .sign_with_keys(keys)
            .expect("sign with test key");
        event.as_json()
    }

    #[test]
    fn rejected_connect_reports_failed_state() {
        let _g = SERIAL.lock().unwrap();
        DISPATCHED.lock().unwrap().clear();
        let (tx, rx) = mpsc::channel();
        let driver = make_driver(tx);

        driver.signin(None);
        let _awaiting = rx.try_recv().expect("awaiting state");
        let connect_request = last_dispatched_request();

        let reply = ExternalSignerResponse {
            correlation_id: connect_request.correlation_id,
            outcome: ExternalSignerOutcome::Rejected {
                reason: "user cancelled".to_string(),
            },
            signer_package: None,
        };
        driver.deliver(&serde_json::to_string(&reply).unwrap());

        match rx.try_recv().expect("failed state sent") {
            ActorCommand::Nip55SignerStateChanged { state, reason } => {
                assert_eq!(state, "failed");
                assert_eq!(reason.as_deref(), Some("user cancelled"));
            }
            other => panic!("expected failed state, got {other:?}"),
        }
        assert!(
            rx.try_recv().is_err(),
            "no AddSigner on a rejected connect"
        );
    }

    #[test]
    fn malformed_response_is_dropped_silently() {
        let _g = SERIAL.lock().unwrap();
        let (tx, rx) = mpsc::channel();
        let driver = make_driver(tx);
        driver.deliver("not-json");
        assert!(rx.try_recv().is_err(), "malformed response sends nothing");
    }

    #[test]
    fn restore_reconstructs_signer_without_interaction() {
        let _g = SERIAL.lock().unwrap();
        DISPATCHED.lock().unwrap().clear();
        let (tx, rx) = mpsc::channel();
        let driver = make_driver(tx);

        let keys = nostr::Keys::generate();
        // `SignerPayload` serde form: `{"kind":"nip55","body":{…}}` — the
        // exact JSON `persistence_payload_json()` produced and the actor
        // persisted through the keyring capability.
        let payload = serde_json::json!({
            "kind": "nip55",
            "body": {
                "user_pubkey_hex": keys.public_key().to_hex(),
                "signer_package": "com.greenart7c3.nostrsigner",
                "granted_permissions": ["sign_event:1", "nip44_encrypt"],
            },
        });
        driver.restore(&payload.to_string());

        match rx.try_recv().expect("AddSigner sent") {
            ActorCommand::AddSigner {
                source: nmp_core::SignerSource::RemoteHandle(handle),
                make_active,
            } => {
                assert!(make_active);
                assert_eq!(handle.signer_kind(), "nip55");
                assert_eq!(handle.pubkey_hex(), keys.public_key().to_hex());
            }
            other => panic!("expected AddSigner, got {other:?}"),
        }
        assert!(
            DISPATCHED.lock().unwrap().is_empty(),
            "restore must not require a host round-trip"
        );
    }
}
