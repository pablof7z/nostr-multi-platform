//! Unit tests for `Nip55Signer`.
//!
//! Split into cohesive submodules by what they test (AGENTS.md file-size
//! rule — this directory replaces the single `tests.rs` file):
//! - [`lifecycle`] — construction, identity (`signer_kind`/`pubkey_hex`),
//!   `op_timeout`, the sign round-trip, and the outbound request shape.
//! - [`connect_and_permissions`] — first-connect fast-path routing: restored
//!   grants, the single-Activity-slot interactive rejection, the
//!   `ContentResolver` → forced-interactive retry, and `signer_package`
//!   learned from a response.
//! - [`delivery`] — `deliver_external_response` outcomes (rejected /
//!   unavailable / unknown correlation id / malformed JSON), `disconnect`,
//!   transport-send failure cleanup, and independent multi-op resolution.
//! - [`persistence_and_crypto`] — `nip44_encrypt`/`nip44_decrypt` round
//!   trips, and the `to_payload`/`from_payload` persistence round-trip
//!   (including the empty-grant-batch restore path, issue #2523).
//!
//! This file (`mod.rs`) owns the shared test harness every submodule uses:
//! `FakeExternalSignerTransport` (captures outbound requests, lets tests
//! inject responses synchronously) and the `make_signed_event_json` /
//! `make_signer_with_pubkey` builders.
//!
//! NOTE: NIP-55 does NOT perform real cryptography — the signer app (Amber)
//! holds the key. Tests inject a `FakeExternalSignerTransport` that returns a
//! pre-built signed event JSON body (produced by a local key) as the `Ok`
//! result, making the mapper verify a real signature.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_signer_iface::RemoteSignerHandle;
use nmp_signer_iface::UnsignedEvent;
use nmp_signer_iface::{
    ExternalSignerOutcome, ExternalSignerRequest, ExternalSignerResponse, ExternalSignerTransport,
    Nip55Permission, SignerError, SignerOp, EXTERNAL_SIGN_TIMEOUT,
};
use nostr::PublicKey;

use crate::signers::nip55::Nip55Signer;
use crate::signers::payload::{Nip55Payload, SignerPayload};
use crate::signers::traits::Signer;
use crate::LocalKeySigner;

mod connect_and_permissions;
mod delivery;
mod lifecycle;
mod persistence_and_crypto;

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

    /// Drain and return all captured requests (oldest first). Called from
    /// `overlapping_interactive_sign_requests_are_rejected`.
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
