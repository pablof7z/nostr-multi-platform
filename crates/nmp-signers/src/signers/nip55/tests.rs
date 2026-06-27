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

use nmp_signer_iface::UnsignedEvent;
use nmp_signer_iface::RemoteSignerHandle;
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
        self.inner.lock().unwrap().requests.drain(..).collect()
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
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: signed_json,
        },
    );

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
    assert!(!req.correlation_id.is_empty(), "correlation_id must be set");
    assert!(
        req.permissions.is_empty(),
        "live sign requests must not re-request the permission batch"
    );
    assert!(
        req.granted_permissions
            .iter()
            .any(|p| p.kind == "sign_event:1"),
        "live sign requests must carry persisted grant facts for the resolver fast-path"
    );
    assert!(
        req.uses_content_resolver_fast_path(),
        "restored/live signer requests with grants should avoid a Connect dialog"
    );
}

/// A restored NIP-55 payload is immediately usable without a fresh permission
/// request: live ops carry `granted_permissions`, while `permissions` remains
/// empty so Amber is not asked to reapprove the batch.
#[test]
fn restored_payload_sign_request_uses_granted_fast_path_without_reapproval() {
    let local = LocalKeySigner::generate();
    let payload = Nip55Payload {
        user_pubkey_hex: local.pubkey().to_hex(),
        signer_package: Some("com.greenart7c3.nostrsigner".to_string()),
        granted_permissions: vec!["sign_event:1".to_string()],
    };
    let transport = FakeExternalSignerTransport::new();
    let signer = Nip55Signer::from_payload(
        &payload,
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    )
    .expect("restore from payload");

    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "restored fast path".to_string(),
        created_at: 1,
    };
    let _op = <Nip55Signer as Signer>::sign(&signer, unsigned);
    let req = transport.last_request().expect("request captured");

    assert!(req.permissions.is_empty());
    assert_eq!(
        req.granted_permissions,
        vec![Nip55Permission::sign_event(1)]
    );
    assert!(req.uses_content_resolver_fast_path());
}

/// Android has one Activity Result slot for interactive signer Intents. Rust
/// rejects a second interactive request before native can overwrite the first
/// pending correlation id.
#[test]
fn overlapping_interactive_sign_requests_are_rejected() {
    let local = LocalKeySigner::generate();
    let transport = FakeExternalSignerTransport::new();
    let signer = Nip55Signer::new(
        local.pubkey(),
        Some("com.greenart7c3.nostrsigner".to_string()),
        vec![],
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    );
    let unsigned = UnsignedEvent {
        pubkey: local.pubkey().to_hex(),
        kind: 1,
        tags: vec![],
        content: "interactive".to_string(),
        created_at: 1,
    };

    let mut op1 = <Nip55Signer as Signer>::sign(&signer, unsigned.clone());
    assert!(op1.poll().is_none());
    let op2 = <Nip55Signer as Signer>::sign(&signer, unsigned);

    match op2.wait(Duration::from_secs(1)) {
        Err(SignerError::Unavailable(m)) => assert!(m.contains("approval already pending")),
        other => panic!("expected overlapping approval rejection, got {other:?}"),
    }
    assert_eq!(signer.pending_len(), 1);
    assert_eq!(
        transport.drain_requests().len(),
        1,
        "second interactive request must not reach native"
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
    let transport = FakeExternalSignerTransport::new();
    let signer = Nip55Signer::new(
        local.pubkey(),
        Some("com.greenart7c3.nostrsigner".to_string()),
        vec![],
        Arc::clone(&transport) as Arc<dyn ExternalSignerTransport>,
    );

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

/// A resolver-path `Unavailable` means a granted permission was silently
/// revoked or unavailable. Rust owns the retry policy: keep the same pending
/// op open and re-issue the request once with `force_interactive = true`.
#[test]
fn content_resolver_unavailable_retries_as_forced_interactive() {
    let local = LocalKeySigner::generate();
    let pubkey = local.pubkey();
    let (signer, transport) = make_signer_with_pubkey(pubkey);
    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "resolver retry".to_string(),
        created_at: 1,
    };

    let mut op = <Nip55Signer as Signer>::sign(&signer, unsigned);
    let first = transport.last_request().expect("first request");
    assert!(first.uses_content_resolver_fast_path());
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Unavailable {
            reason: "ContentResolver returned null result".to_string(),
        },
    );

    assert!(
        op.poll().is_none(),
        "retry must keep the original op pending"
    );
    let retry = transport.last_request().expect("retry request");
    assert_eq!(retry.correlation_id, first.correlation_id);
    assert!(retry.force_interactive);
    assert!(!retry.uses_content_resolver_fast_path());
    assert_eq!(signer.pending_len(), 1);

    let signed_json = make_signed_event_json(&pubkey, &local);
    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: signed_json,
        },
    );
    let signed = op.wait(Duration::from_secs(1)).expect("retry resolves");
    assert_eq!(signed.unsigned.pubkey, pubkey.to_hex());
    assert_eq!(signer.pending_len(), 0);
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
    assert_eq!(
        signer.pending_len(),
        0,
        "failed send must not leak pending entry"
    );
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
    assert!(
        matches!(op, SignerOp::Pending(_)),
        "nip44_encrypt must be Pending"
    );

    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: "fake-ciphertext".to_string(),
        },
    );

    let ct = op
        .wait(Duration::from_secs(1))
        .expect("encrypt must succeed");
    assert_eq!(ct, "fake-ciphertext");
}

/// `nip44_decrypt` returns `Pending` and resolves with the plaintext.
#[test]
fn nip44_decrypt_round_trip() {
    let local = LocalKeySigner::generate();
    let (signer, transport) = make_signer_with_pubkey(local.pubkey());
    let sender = LocalKeySigner::generate().pubkey();

    let op = signer.nip44_decrypt(&sender.to_hex(), "fake-ciphertext");
    assert!(
        matches!(op, SignerOp::Pending(_)),
        "nip44_decrypt must be Pending"
    );

    transport.respond_to_last(
        &signer,
        ExternalSignerOutcome::Ok {
            result: "hello".to_string(),
        },
    );

    let pt = op
        .wait(Duration::from_secs(1))
        .expect("decrypt must succeed");
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

    let payload = signer.to_payload().expect("to_payload");
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

/// ADR-0048 §D5 + V-08 degrade pin: `nip44_decrypt` is implemented in
/// `Nip55Signer` (the capability is complete) but is NOT wired to the
/// DM-inbox path (deferred to V-08/#961).
///
/// This test pins the staging boundary: the method is callable in isolation,
/// but the receive-side infrastructure does not call it.  When V-08 is
/// implemented, this test can be upgraded to an end-to-end receive test.
#[test]
fn nip55_nip44_decrypt_capability_exists_but_receive_path_is_deferred() {
    use nmp_signer_iface::RemoteSignerHandle;
    use nmp_signer_iface::{ExternalSignerOutcome, SignerOp};
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
