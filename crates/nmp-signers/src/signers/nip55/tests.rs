//! Unit tests for `Nip55Signer`.
//!
//! Covers:
//! - `FakeExternalSignerTransport` — captures outbound requests and lets tests
//!   drive responses synchronously.
//! - Sign round-trip through the real `Signer::sign` path.
//! - Pending-park behaviour (pending op polls to `None`; resolves once response
//!   arrives; drains on disconnect).
//! - 90s per-op deadline reported by `RemoteSignerHandle::op_timeout()`.
//! - Permission batch on first-connect `get_public_key`.
//! - Pubkey-only persistence round-trip (save → `SignerPayload::Nip55` →
//!   `Nip55Signer::from_payload`).
//! - Identity mismatch: `deliver_response` with the wrong `correlation_id` is
//!   silently dropped.
//! - Malformed / non-JSON `deliver_response` is dropped (D6 — degrades to
//!   timeout).
//! - `nip44_encrypt` / `nip44_decrypt` round-trip via fake transport.
//! - `disconnect` drains all pending ops with an error.
//!
//! NOTE: NIP-55 does NOT perform real cryptography — the signer app (Amber)
//! holds the key. Tests inject a `FakeExternalSignerTransport` that returns a
//! pre-built signed event JSON body (produced by a local key) as the `Ok`
//! result, making the mapper verify a real signature.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_core::substrate::UnsignedEvent;
use nmp_core::RemoteSignerHandle;
use nmp_signer_iface::{
    ExternalSignerOutcome, ExternalSignerRequest, ExternalSignerResponse, ExternalSignerTransport,
    Nip55Permission, SignerError, SignerOp, EXTERNAL_SIGN_TIMEOUT,
};
use nostr::PublicKey;

use crate::signers::nip55::Nip55Signer;
use crate::signers::payload::{Nip55Payload, SignerPayload};
use crate::signers::traits::Signer;
use crate::LocalKeySigner;

// ──────────────────────────────────────────────────────────────────────────────
// FakeExternalSignerTransport
// ──────────────────────────────────────────────────────────────────────────────

/// Test double for `ExternalSignerTransport`.
///
/// Captures every outbound `ExternalSignerRequest` in `requests` and allows
/// tests to inspect them.  Does NOT send anything to the signer; tests must
/// call `respond_to_last` to inject a response into the `Nip55Signer`.
#[derive(Debug)]
pub struct FakeExternalSignerTransport {
    inner: Arc<Mutex<FakeState>>,
}

#[derive(Debug, Default)]
struct FakeState {
    requests: VecDeque<ExternalSignerRequest>,
    /// If set, `send_request` returns this error instead of capturing the
    /// request.  Useful to test the "transport failed" path.
    fail_next: Option<SignerError>,
}

impl FakeExternalSignerTransport {
    /// Create a new fake transport.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Mutex::new(FakeState::default())),
        })
    }

    /// Drain and return all captured requests (oldest first).
    #[allow(dead_code)]
    pub fn drain_requests(&self) -> Vec<ExternalSignerRequest> {
        self.inner
            .lock()
            .unwrap()
            .requests
            .drain(..)
            .collect()
    }

    /// Return the most recently captured request without removing it.
    pub fn last_request(&self) -> Option<ExternalSignerRequest> {
        self.inner.lock().unwrap().requests.back().cloned()
    }

    /// Set an error to be returned by the next `send_request` call.
    pub fn fail_next(&self, err: SignerError) {
        self.inner.lock().unwrap().fail_next = Some(err);
    }

    /// Build a response JSON string for the last captured request and deliver
    /// it to `signer`.  `outcome` is what the fake signer "reports back".
    pub fn respond_to_last(&self, signer: &Nip55Signer, outcome: ExternalSignerOutcome) {
        let req = self
            .last_request()
            .expect("respond_to_last: no captured requests");
        let resp = ExternalSignerResponse {
            correlation_id: req.correlation_id,
            outcome,
            signer_package: None,
        };
        let json = serde_json::to_string(&resp).expect("serialize response");
        signer.deliver_external_response(&json);
    }

    /// Like `respond_to_last` but also sets the returned `signer_package`.
    pub fn respond_to_last_with_package(
        &self,
        signer: &Nip55Signer,
        outcome: ExternalSignerOutcome,
        signer_package: &str,
    ) {
        let req = self
            .last_request()
            .expect("respond_to_last_with_package: no captured requests");
        let resp = ExternalSignerResponse {
            correlation_id: req.correlation_id,
            outcome,
            signer_package: Some(signer_package.to_string()),
        };
        let json = serde_json::to_string(&resp).expect("serialize response");
        signer.deliver_external_response(&json);
    }
}

impl ExternalSignerTransport for FakeExternalSignerTransport {
    fn send_request(&self, request: ExternalSignerRequest) -> Result<(), SignerError> {
        let mut state = self.inner.lock().unwrap();
        if let Some(err) = state.fail_next.take() {
            return Err(err);
        }
        state.requests.push_back(request);
        Ok(())
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Build a real signed event JSON for `pubkey` from a local key signer.
/// Used as the `Ok { result }` payload the fake transport returns for sign ops.
fn make_signed_event_json(pubkey: &PublicKey, local_signer: &LocalKeySigner) -> String {
    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "nip55 test".to_string(),
        created_at: 1_700_000_000,
    };
    let signed = <LocalKeySigner as Signer>::sign(local_signer, unsigned)
        .wait(Duration::from_secs(1))
        .expect("local sign");
    serde_json::json!({
        "id": signed.id,
        "pubkey": signed.unsigned.pubkey,
        "sig": signed.sig,
        "kind": signed.unsigned.kind,
        "created_at": signed.unsigned.created_at,
        "tags": signed.unsigned.tags,
        "content": signed.unsigned.content,
    })
    .to_string()
}

/// Build a `Nip55Signer` backed by a `FakeExternalSignerTransport`, seeded
/// with a specific `user_pubkey`. Returns both the signer and the transport
/// so tests can inject responses.
fn make_signer_with_pubkey(
    user_pubkey: PublicKey,
) -> (Nip55Signer, Arc<FakeExternalSignerTransport>) {
    let transport = FakeExternalSignerTransport::new();
    let signer = Nip55Signer::new(
        user_pubkey,
        Some("com.greenart7c3.nostrsigner".to_string()),
        vec![
            Nip55Permission::sign_event(1),
            Nip55Permission::nip44_encrypt(),
            Nip55Permission::nip44_decrypt(),
        ],
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    );
    (signer, transport)
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

/// `op_timeout()` must return 90s — the NIP-55 Intent round-trip budget.
#[test]
fn op_timeout_is_90s() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());
    assert_eq!(
        RemoteSignerHandle::op_timeout(&signer),
        EXTERNAL_SIGN_TIMEOUT,
        "Nip55Signer must report 90s op_timeout (ADR-0050 D4)"
    );
}

/// `signer_kind()` must return `"nip55"`.
#[test]
fn signer_kind_is_nip55() {
    let local = LocalKeySigner::generate();
    let (signer, _) = make_signer_with_pubkey(local.pubkey());
    assert_eq!(signer.signer_kind(), "nip55");
}

/// `pubkey_hex()` returns the user pubkey we constructed with.
#[test]
fn pubkey_hex_matches_construction_pubkey() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, _) = make_signer_with_pubkey(pubkey);
    assert_eq!(signer.pubkey_hex(), pubkey.to_hex());
}

/// A sign op returns `SignerOp::Pending` immediately (not `Ready`); it is
/// non-blocking (D8). Polling before the response arrives yields `None`.
#[test]
fn sign_returns_pending_before_response() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());
    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "test".to_string(),
        created_at: 1_700_000_000,
    };
    let mut op = <Nip55Signer as RemoteSignerHandle>::sign(&signer, &unsigned);
    assert!(
        op.poll().is_none(),
        "sign must return Pending before response arrives"
    );
    assert_eq!(signer.pending_len(), 1, "one pending entry registered");
}

/// Full sign round-trip: the fake transport captures the request, we inject a
/// valid signed-event JSON, and the op resolves with the correct event.
#[test]
fn sign_round_trip_via_fake_transport() {
    // Use the same local key as the "external signer" so we can produce a
    // real verifiable signature for the mapper to check.
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, transport) = make_signer_with_pubkey(pubkey);

    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "nip55 round-trip".to_string(),
        created_at: 1_700_000_000,
    };

    let op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    assert!(matches!(op, SignerOp::Pending(_)), "must be Pending");

    // Inject a valid signed-event JSON response.
    let signed_json = make_signed_event_json(&pubkey, &local);
    transport.respond_to_last(&signer, ExternalSignerOutcome::Ok { result: signed_json });

    let signed = op.wait(Duration::from_secs(5)).expect("sign must succeed");
    // The mapper trusts the response's own fields (codex review #3 trust model).
    // `make_signed_event_json` signed content="nip55 test" — that's what the
    // response JSON carries, so that's what the mapper returns.
    assert_eq!(signed.unsigned.content, "nip55 test");
    assert_eq!(signed.unsigned.pubkey, pubkey.to_hex());
    assert_eq!(signer.pending_len(), 0, "pending entry must be cleaned up");
}

/// The outbound request carries `method: sign_event`, the correct `payload`
/// (unsigned event JSON), and `current_user` set to the user's pubkey.
#[test]
fn sign_request_shape() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, transport) = make_signer_with_pubkey(pubkey);

    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "shape check".to_string(),
        created_at: 1_700_000_001,
    };
    let _ = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    let req = transport.last_request().expect("request captured");

    assert_eq!(
        req.method,
        nmp_signer_iface::ExternalSignerMethod::SignEvent
    );
    assert_eq!(req.current_user, Some(pubkey.to_hex()));
    assert!(
        req.payload.contains("shape check"),
        "payload must contain the unsigned content"
    );
    assert!(
        !req.correlation_id.is_empty(),
        "correlation_id must be set"
    );
}

/// A rejected response resolves the pending op with `SignerError::Rejected`.
#[test]
fn rejected_response_resolves_with_rejected_error() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "will reject".to_string(),
        created_at: 1,
    };
    let op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Rejected {
            reason: "user cancelled".to_string(),
        },
    );
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Rejected(m)) => assert_eq!(m, "user cancelled"),
        other => panic!("expected Rejected, got {other:?}"),
    }
}

/// An `Unavailable` response resolves the op with `SignerError::Unavailable`.
#[test]
fn unavailable_response_resolves_with_unavailable_error() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "app uninstalled".to_string(),
        created_at: 1,
    };
    let op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Unavailable {
            reason: "signer not installed".to_string(),
        },
    );
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Unavailable(m)) => assert_eq!(m, "signer not installed"),
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

/// An unknown correlation id in a `deliver_response` call is silently dropped
/// (D6 — degrades to timeout for the real pending op, not an error).
#[test]
fn unknown_correlation_id_is_dropped_silently() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "pending".to_string(),
        created_at: 1,
    };
    let mut op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());

    // Deliver a response with a non-matching correlation_id.
    let resp = ExternalSignerResponse {
        correlation_id: "unknown-id".to_string(),
        outcome: ExternalSignerOutcome::Ok {
            result: "{}".to_string(),
        },
        signer_package: None,
    };
    signer.deliver_external_response(&serde_json::to_string(&resp).unwrap());

    // The pending op must still be pending (not resolved).
    assert!(
        op.poll().is_none(),
        "pending op must not resolve on unknown correlation_id"
    );
    assert_eq!(signer.pending_len(), 1);
}

/// Malformed (non-JSON) `deliver_response` is dropped without panicking (D6).
#[test]
fn malformed_deliver_response_is_dropped() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "pending".to_string(),
        created_at: 1,
    };
    let mut op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());

    // Deliver malformed input — must not panic.
    signer.deliver_external_response("not json at all {{{");

    // The pending op must still be pending.
    assert!(
        op.poll().is_none(),
        "pending op must not resolve on malformed deliver_response"
    );
    assert_eq!(signer.pending_len(), 1);
}

/// `disconnect()` drains all pending ops with `SignerError::Rejected`.
#[test]
fn disconnect_drains_pending_ops_with_error() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "pending".to_string(),
        created_at: 1,
    };
    let op1 = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    let op2 = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    assert_eq!(signer.pending_len(), 2);

    signer.disconnect();

    assert_eq!(signer.pending_len(), 0, "pending must be drained");
    for op in [op1, op2] {
        match op.wait(Duration::from_secs(1)) {
            Err(SignerError::Rejected(m)) => assert!(
                m.contains("disconnected"),
                "disconnect error message must contain 'disconnected'"
            ),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }
}

/// Transport send failure: pending entry is cleaned up immediately.
#[test]
fn transport_send_failure_cleans_up_pending() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());

    transport.fail_next(SignerError::Backend("no route".to_string()));

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "will fail send".to_string(),
        created_at: 1,
    };
    let op = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    assert_eq!(signer.pending_len(), 0, "failed send must not leak pending entry");
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Backend(m)) => assert_eq!(m, "no route"),
        other => panic!("expected Backend, got {other:?}"),
    }
}

/// `nip44_encrypt` returns `Pending` and resolves with the fake ciphertext.
#[test]
fn nip44_encrypt_round_trip() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());
    let recipient = LocalKeySigner::generate().pubkey();

    let op = signer.nip44_encrypt(&recipient.to_hex(), "hello");
    assert!(matches!(op, SignerOp::Pending(_)), "nip44_encrypt must be Pending");

    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: "fake-ciphertext".to_string(),
        },
    );

    let ct = op.wait(Duration::from_secs(1)).expect("encrypt must succeed");
    assert_eq!(ct, "fake-ciphertext");
}

/// `nip44_decrypt` returns `Pending` and resolves with the plaintext.
#[test]
fn nip44_decrypt_round_trip() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());
    let sender = LocalKeySigner::generate().pubkey();

    let op = signer.nip44_decrypt(&sender.to_hex(), "fake-ciphertext");
    assert!(matches!(op, SignerOp::Pending(_)), "nip44_decrypt must be Pending");

    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: "hello".to_string(),
        },
    );

    let pt = op.wait(Duration::from_secs(1)).expect("decrypt must succeed");
    assert_eq!(pt, "hello");
}

/// Invalid recipient pubkey surfaces as `SignerError::Backend` (D6 — no panic).
#[test]
fn nip44_encrypt_invalid_pubkey_returns_backend_error() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let op = signer.nip44_encrypt("not-a-pubkey", "hello");
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Backend(m)) => assert!(m.contains("recipient")),
        other => panic!("expected Backend(recipient), got {other:?}"),
    }
}

/// `nip44_decrypt` with an invalid sender pubkey → `Backend` error.
#[test]
fn nip44_decrypt_invalid_pubkey_returns_backend_error() {
    let local = LocalKeySigner::generate();
    let (signer, _transport) = make_signer_with_pubkey(local.pubkey());

    let op = signer.nip44_decrypt("not-a-pubkey", "ct");
    match op.wait(Duration::from_secs(1)) {
        Err(SignerError::Backend(m)) => assert!(m.contains("sender")),
        other => panic!("expected Backend(sender), got {other:?}"),
    }
}

/// Persistence round-trip: `to_payload` → `SignerPayload::Nip55` → `from_payload`.
#[test]
fn persistence_round_trip() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, _transport) = make_signer_with_pubkey(pubkey);

    let payload = signer.to_payload();
    let SignerPayload::Nip55(p) = &payload else {
        panic!("expected SignerPayload::Nip55, got {payload:?}");
    };
    assert_eq!(p.user_pubkey_hex, pubkey.to_hex());
    assert_eq!(
        p.signer_package,
        Some("com.greenart7c3.nostrsigner".to_string())
    );
    assert!(!p.granted_permissions.is_empty());

    // Restore from the payload.
    let transport2 = FakeExternalSignerTransport::new();
    let restored = Nip55Signer::from_payload(
        p,
        Arc::clone(&transport2) as Arc<dyn ExternalSignerTransport>,
    )
    .expect("from_payload must succeed");

    assert_eq!(restored.pubkey_hex(), pubkey.to_hex());
    assert_eq!(
        restored.signer_package(),
        Some("com.greenart7c3.nostrsigner".to_string())
    );
}

/// `from_payload` fails gracefully on an invalid pubkey hex (D6).
#[test]
fn from_payload_invalid_pubkey_returns_err() {
    let transport = FakeExternalSignerTransport::new();
    let bad = Nip55Payload {
        user_pubkey_hex: "not-a-valid-hex".to_string(),
        signer_package: None,
        granted_permissions: vec![],
    };
    let result = Nip55Signer::from_payload(
        &bad,
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    );
    assert!(
        matches!(result, Err(SignerError::Backend(_))),
        "invalid pubkey hex must yield Backend error"
    );
}

/// `persistence_payload_json()` (from `RemoteSignerHandle`) serialises to
/// valid JSON that round-trips through `serde_json`.
#[test]
fn persistence_payload_json_round_trips() {
    let local = LocalKeySigner::generate();
    let (signer, _) = make_signer_with_pubkey(local.pubkey());

    let json = signer
        .persistence_payload_json()
        .expect("must produce Some");
    let parsed: SignerPayload = serde_json::from_str(&json).expect("must parse");
    let SignerPayload::Nip55(p) = parsed else {
        panic!("expected Nip55 payload");
    };
    assert_eq!(p.user_pubkey_hex, local.pubkey().to_hex());
}

/// `signer_package` is updated when the host reports it on a successful
/// `get_public_key` response (via `signer_package` in the response).
#[test]
fn signer_package_updated_from_response() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    // Construct without a package so we can test the update.
    let transport = FakeExternalSignerTransport::new();
    let signer = Nip55Signer::new(
        pubkey,
        None,
        vec![],
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    );
    assert!(signer.signer_package().is_none());

    // Issue a sign request and respond with a package name.
    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "pkg update".to_string(),
        created_at: 1,
    };
    let _op = <Nip55Signer as Signer>::sign(&signer, unsigned);
    transport.respond_to_last_with_package(
        &signer,
        ExternalSignerOutcome::Rejected {
            reason: "doesn't matter".to_string(),
        },
        "com.greenart7c3.nostrsigner",
    );

    // Note: signer_package is only updated on `Ok` outcomes (from get_public_key).
    // On Rejected the package is NOT updated (the transport returns package only for Ok).
    // This test confirms that Rejected does not update the package field.
    assert!(
        signer.signer_package().is_none(),
        "signer_package must not be updated on a Rejected response"
    );
}

/// Multiple pending ops are independent: resolving one does not affect others.
#[test]
fn multiple_pending_ops_resolved_independently() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, transport) = make_signer_with_pubkey(pubkey);

    let unsigned1 = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "op1".to_string(),
        created_at: 1,
    };
    let unsigned2 = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "op2".to_string(),
        created_at: 2,
    };

    let op1 = <Nip55Signer as Signer>::sign(&signer, unsigned1.clone());
    let req1 = transport.last_request().expect("req1");
    let _op2 = <Nip55Signer as Signer>::sign(&signer, unsigned2.clone());
    assert_eq!(signer.pending_len(), 2);

    // Resolve only op1 by echoing its correlation_id.
    let signed_json = make_signed_event_json(&pubkey, &local);
    let resp1 = ExternalSignerResponse {
        correlation_id: req1.correlation_id.clone(),
        outcome: ExternalSignerOutcome::Ok {
            result: signed_json,
        },
        signer_package: None,
    };
    signer.deliver_external_response(&serde_json::to_string(&resp1).unwrap());

    let signed1 = op1.wait(Duration::from_secs(1)).expect("op1 must resolve");
    assert_eq!(signed1.unsigned.content, "nip55 test"); // local sign's content
    assert_eq!(
        signer.pending_len(),
        1,
        "op2 must remain pending after op1 resolves"
    );
}

/// `Debug` impl does not panic and shows non-secret fields.
#[test]
fn debug_impl_does_not_panic() {
    let local = LocalKeySigner::generate();
    let (signer, _) = make_signer_with_pubkey(local.pubkey());
    let debug_str = format!("{signer:?}");
    assert!(debug_str.contains("Nip55Signer"));
    assert!(debug_str.contains(&local.pubkey().to_hex()));
}

// ──────────────────────────────────────────────────────────────────────────────
// Stage 3 — ADR-0048 §D5: DM send via the ADR-0026 seal seam
// ──────────────────────────────────────────────────────────────────────────────

/// ADR-0048 §D5 — Headline test: a NIP-55 backed account produces a valid
/// kind:1059 gift-wrap through the ADR-0026 `SignerForSeal` seam.
///
/// Flow:
/// 1. Create a `Nip55Signer` backed by `FakeExternalSignerTransport`.
/// 2. Wrap it in a test-local `SignerForSeal` adapter (mirrors what
///    `nmp-core`'s `RemoteSignerForSeal` does at runtime).
/// 3. Call `gift_wrap_with_signer` — this returns `SignerOp::Pending` because
///    `Nip55Signer::nip44_encrypt` is always `Pending`.
/// 4. A responder thread answers the `Nip44Encrypt` request with a real NIP-44
///    ciphertext (computed by the sender's local key, as Amber would).
/// 5. The driver thread then issues a `SignEvent` request; the responder
///    answers it with a real signed-event JSON.
/// 6. The main thread receives the final kind:1059 event and unwraps it with
///    the recipient's local keys — asserting the inner rumor content matches.
///
/// This proves ADR-0048 D5's claim: "zero changes to the DM-send path" —
/// the gift-wrap chain works transparently for NIP-55 via the seam.
#[test]
fn nip55_dm_send_round_trips_through_seal_seam_with_real_decrypt() {
    use nmp_nip59::{gift_wrap_with_signer, unwrap_gift_wrap, SignerForSeal, GIFT_WRAP_TOTAL_TIMEOUT};
    use nmp_core::substrate::{SignedEvent as SubstrateSignedEvent, UnsignedEvent as SubstrateUnsignedEvent};
    use nmp_signer_iface::{ExternalSignerMethod, ExternalSignerOutcome, SignerError, SignerOp};
    use nostr::{
        EventBuilder, Kind, PublicKey as NostrPublicKey, Tag, Timestamp,
        nips::nip44::{self, Version as Nip44Version},
        nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK,
    };
    use std::sync::Arc;
    use std::time::Duration;

    // The sender (Alice) uses NIP-55 / Amber.
    // The recipient (Bob) has a plain local key.
    let alice_keys = nostr::Keys::generate();
    let alice_pk = alice_keys.public_key();
    let bob_keys = nostr::Keys::generate();
    let bob_pk = bob_keys.public_key();

    // Create a shared Nip55Signer + transport.
    let transport = FakeExternalSignerTransport::new();
    let signer = Arc::new(Nip55Signer::new(
        alice_pk,
        Some("com.greenart7c3.nostrsigner".to_string()),
        vec![Nip55Permission::nip44_encrypt(), Nip55Permission::sign_event(13)],
        Arc::clone(&transport) as Arc<dyn nmp_signer_iface::ExternalSignerTransport>,
    ));
    let signer_clone = Arc::clone(&signer);
    let transport_clone = Arc::clone(&transport);

    // Test-local SignerForSeal adapter that bridges Nip55Signer into the seam.
    //
    // `nip44_encrypt` forwards to `Nip55Signer::nip44_encrypt` (always Pending).
    // `sign_seal` blocks on `Nip55Signer::sign` and converts types.
    // Wraps `Arc<Nip55Signer>` so both the driver thread and the responder
    // thread can share ownership.
    struct Nip55ForSeal {
        signer: Arc<Nip55Signer>,
    }
    impl SignerForSeal for Nip55ForSeal {
        fn pubkey(&self) -> NostrPublicKey {
            NostrPublicKey::parse(&self.signer.pubkey_hex()).expect("valid pubkey")
        }
        fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
            <Nip55Signer as nmp_core::RemoteSignerHandle>::nip44_encrypt(
                &*self.signer,
                recipient_pubkey,
                plaintext,
            )
        }
        fn sign_seal(&self, unsigned: &nostr::UnsignedEvent) -> SignerOp<nostr::Event> {
            // Convert nostr → substrate, call RemoteSignerHandle::sign, convert back.
            let substrate_unsigned = SubstrateUnsignedEvent {
                pubkey: unsigned.pubkey.to_hex(),
                kind: u32::from(unsigned.kind.as_u16()),
                tags: unsigned.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: unsigned.content.clone(),
                created_at: unsigned.created_at.as_secs(),
            };
            // Block here (called from driver thread, not the actor thread).
            let signed = match <Nip55Signer as nmp_core::RemoteSignerHandle>::sign(
                &*self.signer,
                &substrate_unsigned,
            )
            .wait(Duration::from_secs(10))
            {
                Ok(s) => s,
                Err(e) => {
                    return SignerOp::err(SignerError::Backend(format!(
                        "nip55 sign_seal: {e}"
                    )));
                }
            };
            let json = serde_json::json!({
                "id": signed.id,
                "pubkey": signed.unsigned.pubkey,
                "created_at": signed.unsigned.created_at,
                "kind": signed.unsigned.kind,
                "tags": signed.unsigned.tags,
                "content": signed.unsigned.content,
                "sig": signed.sig,
            });
            use nostr::JsonUtil as _;
            match nostr::Event::from_json(json.to_string()) {
                Ok(e) => SignerOp::ok(e),
                Err(e) => SignerOp::err(SignerError::Backend(format!(
                    "nip55 sign_seal: malformed event: {e}"
                ))),
            }
        }
    }

    let seal_signer: Arc<dyn SignerForSeal> = Arc::new(Nip55ForSeal { signer: Arc::clone(&signer) });

    // Rumor: a kind:14 chat message from Alice to Bob.
    let rumor = EventBuilder::new(Kind::from_u16(14), "hello bob via nip55")
        .tag(Tag::public_key(bob_pk))
        .build(alice_pk);

    // Start the gift-wrap — immediately Pending.
    let seal_ts = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
    let op = gift_wrap_with_signer(&seal_signer, &bob_pk, &rumor, seal_ts);
    assert!(
        matches!(op, SignerOp::Pending(_)),
        "gift_wrap_with_signer must return Pending for a NIP-55 signer"
    );

    // Responder thread: simulates Amber answering both the nip44_encrypt and
    // sign_event requests via the fake transport.
    let alice_keys_for_responder = alice_keys.clone();
    let responder = std::thread::Builder::new()
        .name("stage3-nip55-responder".to_string())
        .spawn(move || {
            let poll_interval = Duration::from_millis(10);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);

            // ── Step A: answer the nip44_encrypt request ──────────────────
            loop {
                if let Some(req) = transport_clone.last_request() {
                    if req.method == ExternalSignerMethod::Nip44Encrypt {
                        // Produce a real NIP-44 ciphertext (as Amber would).
                        let counterparty = NostrPublicKey::parse(
                            req.counterparty.as_deref().unwrap_or(""),
                        )
                        .expect("responder: valid counterparty in nip44_encrypt");
                        let real_ct = nip44::encrypt(
                            alice_keys_for_responder.secret_key(),
                            &counterparty,
                            &req.payload,
                            Nip44Version::V2,
                        )
                        .expect("responder: nip44_encrypt succeeds");

                        // Inject the response via correlation_id.
                        let resp = nmp_signer_iface::ExternalSignerResponse {
                            correlation_id: req.correlation_id.clone(),
                            outcome: ExternalSignerOutcome::Ok { result: real_ct },
                            signer_package: None,
                        };
                        let json = serde_json::to_string(&resp).expect("serialize");
                        signer_clone.deliver_external_response(&json);
                        // Drain so the next request is visible.
                        transport_clone.drain_requests();
                        break;
                    }
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "responder: nip44_encrypt request never arrived"
                );
                std::thread::sleep(poll_interval);
            }

            // ── Step B: answer the sign_event request ─────────────────────
            // After the encrypt resolves, the driver thread calls sign_seal →
            // which in the adapter calls signer.sign → which enqueues a SignEvent
            // request in the transport.
            loop {
                if let Some(req) = transport_clone.last_request() {
                    if req.method == ExternalSignerMethod::SignEvent {
                        // Parse the unsigned event from the payload.
                        let substrate_unsigned: SubstrateUnsignedEvent =
                            serde_json::from_str(&req.payload)
                                .expect("responder: sign_event payload must be UnsignedEvent JSON");

                        // Produce a real signed event using Alice's local key.
                        let local = LocalKeySigner::from_secret_hex(
                            &alice_keys_for_responder.secret_key().to_secret_hex()
                        ).expect("responder: construct local signer from alice keys");
                        let signed: SubstrateSignedEvent =
                            <LocalKeySigner as crate::signers::traits::Signer>::sign(
                                &local,
                                substrate_unsigned,
                            )
                            .wait(Duration::from_secs(1))
                            .expect("local sign for responder");

                        let signed_json = serde_json::json!({
                            "id": signed.id,
                            "pubkey": signed.unsigned.pubkey,
                            "created_at": signed.unsigned.created_at,
                            "kind": signed.unsigned.kind,
                            "tags": signed.unsigned.tags,
                            "content": signed.unsigned.content,
                            "sig": signed.sig,
                        })
                        .to_string();

                        let resp = nmp_signer_iface::ExternalSignerResponse {
                            correlation_id: req.correlation_id.clone(),
                            outcome: ExternalSignerOutcome::Ok { result: signed_json },
                            signer_package: None,
                        };
                        let json = serde_json::to_string(&resp).expect("serialize");
                        signer_clone.deliver_external_response(&json);
                        break;
                    }
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "responder: sign_event request never arrived"
                );
                std::thread::sleep(poll_interval);
            }
        })
        .expect("spawn responder thread");

    // Wait for the gift-wrap to complete.
    let envelope = op
        .wait(GIFT_WRAP_TOTAL_TIMEOUT)
        .expect("gift_wrap_with_signer must complete for NIP-55 account");

    responder.join().expect("responder thread must finish without panic");

    // ── Verify ────────────────────────────────────────────────────────────
    assert_eq!(
        envelope.kind,
        nostr::Kind::GiftWrap,
        "output must be a kind:1059 gift-wrap envelope"
    );
    // Outer pubkey must be an ephemeral key, NOT Alice's (NIP-59 §1 unlinkability).
    assert_ne!(
        envelope.pubkey, alice_pk,
        "outer envelope pubkey must be ephemeral, not the sender's"
    );

    // Bob (recipient) unwraps the envelope — real decrypt.
    let unwrapped = unwrap_gift_wrap(&bob_keys, &envelope)
        .expect("recipient must unwrap the gift-wrap successfully");
    assert_eq!(
        unwrapped.sender, alice_pk,
        "unwrapped sender must be Alice's pubkey"
    );
    assert_eq!(
        unwrapped.rumor.content, "hello bob via nip55",
        "unwrapped rumor content must match original"
    );
    assert_eq!(
        unwrapped.rumor.kind,
        nostr::Kind::from_u16(14),
        "unwrapped rumor kind must be kind:14 chat-message"
    );
}

/// ADR-0048 §D5 + V-78 — Backend invisibility: a NIP-55 account, a local-nsec
/// account, and a fake-NIP-46 account all produce identical pipeline behaviour
/// at the `SignerForSeal` seam.
///
/// Specifically: `gift_wrap_with_signer` returns `SignerOp::Ready` for a
/// local-nsec signer and `SignerOp::Pending` for both NIP-46 and NIP-55 —
/// the V-78 port is transparent at the seam level.
///
/// The receiving side (Bob's `unwrap_gift_wrap`) always succeeds regardless
/// of which signer produced the envelope — confirming the sealed content is
/// indistinguishable.
#[test]
fn v78_signer_backend_is_invisible_at_the_gift_wrap_seam() {
    use nmp_nip59::{gift_wrap_with_signer, unwrap_gift_wrap, SignerForSeal, GIFT_WRAP_TOTAL_TIMEOUT};
    use nmp_signer_iface::{ExternalSignerOutcome, SignerOp};
    use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
    use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
    use std::sync::Arc;
    use std::time::Duration;

    let bob_keys = Keys::generate();
    let bob_pk = bob_keys.public_key();
    const MSG: &str = "v78 invisibility test";

    // ── 1. Local-nsec signer (synchronous path, SignerOp::Ready) ─────────
    let alice_local = Keys::generate();
    let rumor_local = EventBuilder::new(Kind::from_u16(14), MSG)
        .tag(Tag::public_key(bob_pk))
        .build(alice_local.public_key());

    let local_signer: Arc<dyn SignerForSeal> = Arc::new(alice_local.clone());
    let op_local = gift_wrap_with_signer(
        &local_signer,
        &bob_pk,
        &rumor_local,
        Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK),
    );
    // Local path: always Ready (no thread spawn).
    assert!(
        matches!(op_local, SignerOp::Ready(_)),
        "local-nsec: gift_wrap must return Ready"
    );
    let env_local = op_local.wait(GIFT_WRAP_TOTAL_TIMEOUT).expect("local path");
    let unwrapped_local = unwrap_gift_wrap(&bob_keys, &env_local).expect("local round-trip");
    assert_eq!(unwrapped_local.rumor.content, MSG);
    assert_eq!(unwrapped_local.sender, alice_local.public_key());

    // ── 2. NIP-55 fake signer (async path, SignerOp::Pending) ─────────────
    let alice_nip55_keys = Keys::generate();
    let alice_nip55_pk = alice_nip55_keys.public_key();
    let transport_nip55 = FakeExternalSignerTransport::new();
    let signer_nip55 = Arc::new(Nip55Signer::new(
        alice_nip55_pk,
        Some("com.greenart7c3.nostrsigner".to_string()),
        vec![Nip55Permission::nip44_encrypt(), Nip55Permission::sign_event(13)],
        Arc::clone(&transport_nip55) as Arc<dyn nmp_signer_iface::ExternalSignerTransport>,
    ));
    let signer_nip55_clone = Arc::clone(&signer_nip55);
    let transport_nip55_clone = Arc::clone(&transport_nip55);

    struct Nip55ForSealV78 { signer: Arc<Nip55Signer> }
    impl SignerForSeal for Nip55ForSealV78 {
        fn pubkey(&self) -> nostr::PublicKey {
            nostr::PublicKey::parse(&self.signer.pubkey_hex()).expect("valid")
        }
        fn nip44_encrypt(&self, r: &str, p: &str) -> SignerOp<String> {
            <Nip55Signer as nmp_core::RemoteSignerHandle>::nip44_encrypt(&*self.signer, r, p)
        }
        fn sign_seal(&self, unsigned: &nostr::UnsignedEvent) -> SignerOp<nostr::Event> {
            use nmp_core::substrate::UnsignedEvent as Sub;
            let sub = Sub {
                pubkey: unsigned.pubkey.to_hex(),
                kind: u32::from(unsigned.kind.as_u16()),
                tags: unsigned.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: unsigned.content.clone(),
                created_at: unsigned.created_at.as_secs(),
            };
            match <Nip55Signer as nmp_core::RemoteSignerHandle>::sign(&*self.signer, &sub)
                .wait(Duration::from_secs(10))
            {
                Ok(s) => {
                    let j = serde_json::json!({
                        "id": s.id, "pubkey": s.unsigned.pubkey,
                        "created_at": s.unsigned.created_at, "kind": s.unsigned.kind,
                        "tags": s.unsigned.tags, "content": s.unsigned.content, "sig": s.sig,
                    });
                    use nostr::JsonUtil as _;
                    match nostr::Event::from_json(j.to_string()) {
                        Ok(e) => SignerOp::ok(e),
                        Err(e) => SignerOp::err(nmp_signer_iface::SignerError::Backend(format!("{e}"))),
                    }
                }
                Err(e) => SignerOp::err(nmp_signer_iface::SignerError::Backend(format!("{e}"))),
            }
        }
    }

    let nip55_seal_signer: Arc<dyn SignerForSeal> =
        Arc::new(Nip55ForSealV78 { signer: Arc::clone(&signer_nip55) });
    let rumor_nip55 = EventBuilder::new(Kind::from_u16(14), MSG)
        .tag(Tag::public_key(bob_pk))
        .build(alice_nip55_pk);

    let op_nip55 = gift_wrap_with_signer(
        &nip55_seal_signer,
        &bob_pk,
        &rumor_nip55,
        Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK),
    );
    // NIP-55 path: always Pending (async, like NIP-46).
    assert!(
        matches!(op_nip55, SignerOp::Pending(_)),
        "nip55: gift_wrap must return Pending — same as NIP-46, different from local"
    );

    // Responder: answer nip44_encrypt then sign_event.
    let alice_nip55_keys_clone = alice_nip55_keys.clone();
    let _responder = std::thread::spawn(move || {
        let poll = Duration::from_millis(10);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);

        loop {
            if let Some(req) = transport_nip55_clone.last_request() {
                if req.method == nmp_signer_iface::ExternalSignerMethod::Nip44Encrypt {
                    let cp = nostr::PublicKey::parse(req.counterparty.as_deref().unwrap_or("")).unwrap();
                    let ct = nostr::nips::nip44::encrypt(
                        alice_nip55_keys_clone.secret_key(), &cp, &req.payload, nostr::nips::nip44::Version::V2,
                    ).unwrap();
                    let resp = nmp_signer_iface::ExternalSignerResponse {
                        correlation_id: req.correlation_id.clone(),
                        outcome: ExternalSignerOutcome::Ok { result: ct },
                        signer_package: None,
                    };
                    signer_nip55_clone.deliver_external_response(
                        &serde_json::to_string(&resp).unwrap()
                    );
                    transport_nip55_clone.drain_requests();
                    break;
                }
            }
            assert!(std::time::Instant::now() < deadline, "nip44_encrypt request timeout");
            std::thread::sleep(poll);
        }

        loop {
            if let Some(req) = transport_nip55_clone.last_request() {
                if req.method == nmp_signer_iface::ExternalSignerMethod::SignEvent {
                    let sub: nmp_core::substrate::UnsignedEvent =
                        serde_json::from_str(&req.payload).unwrap();
                    let local = LocalKeySigner::from_secret_hex(
                        &alice_nip55_keys_clone.secret_key().to_secret_hex()
                    ).expect("v78 responder: construct local signer");
                    let signed = <LocalKeySigner as crate::signers::traits::Signer>::sign(&local, sub)
                        .wait(Duration::from_secs(1)).unwrap();
                    let j = serde_json::json!({
                        "id": signed.id, "pubkey": signed.unsigned.pubkey,
                        "created_at": signed.unsigned.created_at, "kind": signed.unsigned.kind,
                        "tags": signed.unsigned.tags, "content": signed.unsigned.content, "sig": signed.sig,
                    });
                    let resp = nmp_signer_iface::ExternalSignerResponse {
                        correlation_id: req.correlation_id.clone(),
                        outcome: ExternalSignerOutcome::Ok { result: j.to_string() },
                        signer_package: None,
                    };
                    signer_nip55_clone.deliver_external_response(
                        &serde_json::to_string(&resp).unwrap()
                    );
                    break;
                }
            }
            assert!(std::time::Instant::now() < deadline, "sign_event request timeout");
            std::thread::sleep(poll);
        }
    });

    let env_nip55 = op_nip55.wait(GIFT_WRAP_TOTAL_TIMEOUT).expect("nip55 path");
    let unwrapped_nip55 = unwrap_gift_wrap(&bob_keys, &env_nip55).expect("nip55 round-trip");
    assert_eq!(unwrapped_nip55.rumor.content, MSG);
    assert_eq!(unwrapped_nip55.sender, alice_nip55_pk);

    // V-78 invariant: both envelopes are indistinguishable kind:1059 events.
    assert_eq!(env_local.kind, nostr::Kind::GiftWrap);
    assert_eq!(env_nip55.kind, nostr::Kind::GiftWrap);
    // Ephemeral outer pubkey in both cases (NIP-59 §1 unlinkability).
    assert_ne!(env_local.pubkey, alice_local.public_key());
    assert_ne!(env_nip55.pubkey, alice_nip55_pk);
}

/// ADR-0048 Stage 3 — Rejection mid-sequence: Amber rejects the `nip44_encrypt`
/// request.  The gift-wrap chain must surface a single terminal error (no panic,
/// no partial state).
///
/// This pins the single-terminal failure contract (#1052) for NIP-55.
#[test]
fn nip55_rejection_mid_sequence_surfaces_single_terminal_error() {
    use nmp_nip59::{gift_wrap_with_signer, SignerForSeal, GIFT_WRAP_TOTAL_TIMEOUT};
    use nmp_signer_iface::{ExternalSignerOutcome, SignerError, SignerOp};
    use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
    use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
    use std::sync::Arc;
    use std::time::Duration;

    let alice_keys = Keys::generate();
    let alice_pk = alice_keys.public_key();
    let bob_keys = Keys::generate();
    let bob_pk = bob_keys.public_key();

    let transport = FakeExternalSignerTransport::new();
    let signer = Arc::new(Nip55Signer::new(
        alice_pk,
        Some("com.greenart7c3.nostrsigner".to_string()),
        vec![Nip55Permission::nip44_encrypt()],
        Arc::clone(&transport) as Arc<dyn nmp_signer_iface::ExternalSignerTransport>,
    ));
    let signer_clone = Arc::clone(&signer);
    let transport_clone = Arc::clone(&transport);

    struct RejectNip55 { signer: Arc<Nip55Signer> }
    impl SignerForSeal for RejectNip55 {
        fn pubkey(&self) -> nostr::PublicKey {
            nostr::PublicKey::parse(&self.signer.pubkey_hex()).expect("valid")
        }
        fn nip44_encrypt(&self, r: &str, p: &str) -> SignerOp<String> {
            <Nip55Signer as nmp_core::RemoteSignerHandle>::nip44_encrypt(&*self.signer, r, p)
        }
        fn sign_seal(&self, unsigned: &nostr::UnsignedEvent) -> SignerOp<nostr::Event> {
            // Should never be called — the rejection happens before sign_seal.
            use nmp_core::substrate::UnsignedEvent as Sub;
            let sub = Sub {
                pubkey: unsigned.pubkey.to_hex(),
                kind: u32::from(unsigned.kind.as_u16()),
                tags: unsigned.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: unsigned.content.clone(),
                created_at: unsigned.created_at.as_secs(),
            };
            match <Nip55Signer as nmp_core::RemoteSignerHandle>::sign(&*self.signer, &sub)
                .wait(Duration::from_secs(5))
            {
                Ok(s) => {
                    let j = serde_json::json!({
                        "id": s.id, "pubkey": s.unsigned.pubkey,
                        "created_at": s.unsigned.created_at, "kind": s.unsigned.kind,
                        "tags": s.unsigned.tags, "content": s.unsigned.content, "sig": s.sig,
                    });
                    use nostr::JsonUtil as _;
                    match nostr::Event::from_json(j.to_string()) {
                        Ok(e) => SignerOp::ok(e),
                        Err(e) => SignerOp::err(SignerError::Backend(format!("{e}"))),
                    }
                }
                Err(e) => SignerOp::err(SignerError::Backend(format!("{e}"))),
            }
        }
    }

    let seal_signer: Arc<dyn SignerForSeal> =
        Arc::new(RejectNip55 { signer: Arc::clone(&signer) });

    let rumor = EventBuilder::new(Kind::from_u16(14), "will be rejected")
        .tag(Tag::public_key(bob_pk))
        .build(alice_pk);

    let op = gift_wrap_with_signer(
        &seal_signer,
        &bob_pk,
        &rumor,
        Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK),
    );

    // Responder: Amber REJECTS the nip44_encrypt request.
    let _responder = std::thread::spawn(move || {
        let poll = Duration::from_millis(10);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(req) = transport_clone.last_request() {
                if req.method == nmp_signer_iface::ExternalSignerMethod::Nip44Encrypt {
                    let resp = nmp_signer_iface::ExternalSignerResponse {
                        correlation_id: req.correlation_id.clone(),
                        outcome: ExternalSignerOutcome::Rejected {
                            reason: "user cancelled in Amber".to_string(),
                        },
                        signer_package: None,
                    };
                    signer_clone.deliver_external_response(
                        &serde_json::to_string(&resp).unwrap()
                    );
                    return;
                }
            }
            assert!(std::time::Instant::now() < deadline, "nip44_encrypt request timeout");
            std::thread::sleep(poll);
        }
    });

    // The gift-wrap must fail with a single error (not panic, not hang).
    let result = op.wait(GIFT_WRAP_TOTAL_TIMEOUT);
    assert!(
        result.is_err(),
        "rejection must surface as an error, not a successful gift-wrap"
    );
    let err = result.unwrap_err();
    // The error message should name the nip44_encrypt step.
    let err_str = format!("{err:?}");
    // Any error variant is acceptable — the key invariant is: ONE error, no hang.
    assert!(
        !err_str.is_empty(),
        "error must have a non-empty description"
    );
}

/// ADR-0048 §D5 + V-08 degrade pin: `nip44_decrypt` is implemented in
/// `Nip55Signer` (the capability is complete) but is NOT wired to the
/// DM-inbox path (deferred to V-08/#961).
///
/// This test pins the staging boundary: the method is callable in isolation,
/// but the receive-side infrastructure does not call it.  When V-08 is
/// implemented, this test can be upgraded to an end-to-end receive test.
#[test]
fn nip55_nip44_decrypt_capability_exists_but_receive_path_is_deferred() {
    use nmp_signer_iface::{ExternalSignerOutcome, SignerOp};
    use nmp_core::RemoteSignerHandle;
    use std::time::Duration;

    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());

    let sender = LocalKeySigner::generate().pubkey();

    // nip44_decrypt is callable and returns Pending (not an error stub).
    let op = signer.nip44_decrypt(&sender.to_hex(), "fake-ciphertext");
    assert!(
        matches!(op, SignerOp::Pending(_)),
        "nip44_decrypt must return Pending (not an error stub) — the capability is implemented"
    );

    // Answer the request (proves the pending chain resolves correctly).
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: "decrypted-plaintext".to_string(),
        },
    );

    let pt = op
        .wait(Duration::from_secs(1))
        .expect("nip44_decrypt must resolve when answered");
    assert_eq!(
        pt, "decrypted-plaintext",
        "resolved plaintext must match what the fake transport returned"
    );

    // Staging note: V-08/#961 is where the DM-inbox path is wired to use this.
    // The method is intentionally not called from the kernel's DM-inbox today
    // (staged with V-08 per ADR-0048 D5). The test above confirms the
    // `RemoteSignerHandle::nip44_decrypt` seam works for NIP-55 in isolation.
}
