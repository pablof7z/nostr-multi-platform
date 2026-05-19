//! Regression tests for the three correctness findings in
//! `docs/perf/codex-reviews/9944bed.md` (codex M6 post-merge review):
//!
//! 1. `Nip07Signer::pubkey()` panic → structured `SignerError::NotReady` at
//!    construction time (D6: errors never cross FFI as panics).
//! 2. NIP-46 sign-event responses verify the schnorr signature + recompute
//!    the event id from the response's own fields (not the local template).
//!    Forged or tampered responses surface as
//!    `SignerError::SignatureVerificationFailed`.
//! 3. `AccountManager::remove(active_id)` clears the active slot atomically,
//!    fires `ActiveChangeObserver` with `current = None`, and is idempotent
//!    on already-removed accounts.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::UnsignedEvent;
use nmp_signers::signers::{Nip46Rpc, Nip46Transport, Nip07Payload};
use nmp_signers::{
    AccountManager, ActiveChangeEvent, ActiveChangeObserver, Kind3RewireObserver,
    LocalKeySigner, Nip07Signer, Nip46SignerHandle, Signer, SignerError,
};

const SAMPLE_PK: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

// ============================================================================
// Fix #1 — Nip07Signer panic → SignerError::NotReady
// ============================================================================

#[test]
fn nip07_from_payload_without_cached_pubkey_returns_not_ready() {
    // Empty payload: no cached pubkey, no wasm handshake possible.  Must
    // surface as structured NotReady, not a panic.
    let payload = Nip07Payload {
        cached_pubkey_hex: None,
    };
    let err = Nip07Signer::from_payload(&payload).expect_err("must refuse empty payload");
    match err {
        SignerError::NotReady(msg) => {
            assert!(
                msg.contains("nip07") && msg.contains("cached pubkey"),
                "NotReady message should mention nip07 + cached pubkey; got: {msg}"
            );
        }
        other => panic!("expected SignerError::NotReady, got {other:?}"),
    }
}

#[test]
fn nip07_from_payload_with_invalid_pubkey_hex_returns_backend_error() {
    // Garbage hex must NOT panic — surfaces as Backend(parse error).
    let payload = Nip07Payload {
        cached_pubkey_hex: Some("not-valid-hex".to_string()),
    };
    let err = Nip07Signer::from_payload(&payload).expect_err("must refuse garbage hex");
    assert!(
        matches!(err, SignerError::Backend(_)),
        "expected Backend(parse error), got {err:?}"
    );
}

#[test]
fn nip07_from_cached_pubkey_then_pubkey_is_infallible() {
    // The construction-gated path: a Nip07Signer that exists at all must
    // always have a real pubkey available synchronously.  This is the
    // post-condition that lets `Signer::pubkey()` honour its trait
    // invariant of "synchronous and infallible after construction succeeds".
    let real_pubkey = LocalKeySigner::generate().pubkey();
    let signer = Nip07Signer::from_cached_pubkey(real_pubkey);
    assert_eq!(signer.pubkey(), real_pubkey);

    // Round-trip via payload: cached pubkey is preserved.
    let payload = signer.to_payload();
    let nmp_signers::SignerPayload::Nip07(np) = payload else {
        panic!("expected nip07 payload");
    };
    assert_eq!(np.cached_pubkey_hex.as_deref(), Some(real_pubkey.to_hex()).as_deref());

    let restored = Nip07Signer::from_payload(&np).expect("restore");
    assert_eq!(restored.pubkey(), real_pubkey);
}

// ============================================================================
// Fix #2 — NIP-46 sign-event response signature verification
// ============================================================================

#[derive(Debug, Default)]
struct StubTransport {
    sent: Mutex<Vec<Nip46Rpc>>,
}

impl Nip46Transport for StubTransport {
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
        self.sent.lock().unwrap().push(rpc);
        Ok(())
    }
}

#[test]
fn nip46_response_with_tampered_signature_is_rejected() {
    // A real keypair plays the role of the remote bunker user.
    let remote_user_signer = LocalKeySigner::generate();
    let remote_user_pubkey = remote_user_signer.pubkey();

    let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com");
    let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
    let transport = Arc::new(StubTransport::default());
    let signer = handle.complete(transport.clone(), remote_user_pubkey);

    let unsigned = UnsignedEvent {
        pubkey: remote_user_pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "honest content".to_string(),
        created_at: 1_700_000_000,
    };
    let mut op = signer.sign(unsigned.clone());

    let rpc_id = {
        let sent = transport.sent.lock().unwrap();
        sent[0].id.clone()
    };

    // Produce a real signed event but **flip one byte of the signature**.
    let real_signed = remote_user_signer
        .sign(unsigned.clone())
        .wait(std::time::Duration::from_secs(1))
        .expect("real sign");
    let mut tampered_sig = real_signed.sig.clone();
    // Replace the last hex char with a different one — same length, still
    // valid hex, but a wrong signature.
    let last_char_pos = tampered_sig
        .char_indices()
        .last()
        .map(|(i, _)| i)
        .expect("non-empty sig");
    let bumped = match tampered_sig.as_bytes()[last_char_pos] {
        b'0'..=b'8' | b'a'..=b'e' => (tampered_sig.as_bytes()[last_char_pos] + 1) as char,
        _ => '0',
    };
    tampered_sig.replace_range(last_char_pos.., &bumped.to_string());

    let response_json = format!(
        r#"{{"id":"{}","pubkey":"{}","sig":"{}","kind":{},"created_at":{},"tags":[],"content":"{}"}}"#,
        real_signed.id,
        real_signed.unsigned.pubkey,
        tampered_sig,
        real_signed.unsigned.kind,
        real_signed.unsigned.created_at,
        real_signed.unsigned.content,
    );
    signer.resolve_response(&rpc_id, Ok(response_json));

    let result = poll_with_timeout(&mut op, std::time::Duration::from_secs(2));
    match result {
        Err(SignerError::SignatureVerificationFailed(msg)) => {
            assert!(
                msg.contains("verify"),
                "expected verify() reference in error, got: {msg}"
            );
        }
        other => panic!("expected SignatureVerificationFailed, got {other:?}"),
    }
}

#[test]
fn nip46_response_with_swapped_content_is_rejected() {
    // Attack scenario: bunker returns a valid signature over event A, but
    // ships event B's content in the response.  The recomputed id won't
    // match the claimed id → verify() rejects.
    let remote_user_signer = LocalKeySigner::generate();
    let remote_user_pubkey = remote_user_signer.pubkey();

    let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com");
    let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
    let transport = Arc::new(StubTransport::default());
    let signer = handle.complete(transport.clone(), remote_user_pubkey);

    let unsigned = UnsignedEvent {
        pubkey: remote_user_pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "honest content".to_string(),
        created_at: 1_700_000_000,
    };
    let mut op = signer.sign(unsigned.clone());
    let rpc_id = transport.sent.lock().unwrap()[0].id.clone();

    let real_signed = remote_user_signer
        .sign(unsigned.clone())
        .wait(std::time::Duration::from_secs(1))
        .expect("real sign");

    // Swap the content but keep the (now-stale) id + sig from the real one.
    let response_json = format!(
        r#"{{"id":"{}","pubkey":"{}","sig":"{}","kind":{},"created_at":{},"tags":[],"content":"{}"}}"#,
        real_signed.id,
        real_signed.unsigned.pubkey,
        real_signed.sig,
        real_signed.unsigned.kind,
        real_signed.unsigned.created_at,
        "ATTACKER REPLACED THIS",
    );
    signer.resolve_response(&rpc_id, Ok(response_json));

    let result = poll_with_timeout(&mut op, std::time::Duration::from_secs(2));
    assert!(
        matches!(result, Err(SignerError::SignatureVerificationFailed(_))),
        "expected SignatureVerificationFailed (id mismatch), got {result:?}"
    );
}

#[test]
fn nip46_response_with_wrong_pubkey_is_rejected_as_mismatch() {
    // A bunker that returns a different pubkey than the cached remote-user
    // pubkey is refused (Mismatch, not SignatureVerificationFailed — the
    // pubkey-identity check runs before sig verification).
    let real_pubkey = LocalKeySigner::generate().pubkey();
    let other_signer = LocalKeySigner::generate();
    let other_pubkey = other_signer.pubkey();

    let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com");
    let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
    let transport = Arc::new(StubTransport::default());
    let signer = handle.complete(transport.clone(), real_pubkey);

    let unsigned = UnsignedEvent {
        pubkey: real_pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "hi".to_string(),
        created_at: 1_700_000_000,
    };
    let mut op = signer.sign(unsigned.clone());
    let rpc_id = transport.sent.lock().unwrap()[0].id.clone();

    // The "wrong" signer signs its own valid event; the response carries
    // the wrong pubkey.
    let other_signed = other_signer
        .sign(UnsignedEvent {
            pubkey: other_pubkey.to_hex(),
            ..unsigned
        })
        .wait(std::time::Duration::from_secs(1))
        .expect("sign");
    let response_json = format!(
        r#"{{"id":"{}","pubkey":"{}","sig":"{}","kind":{},"created_at":{},"tags":[],"content":"{}"}}"#,
        other_signed.id,
        other_signed.unsigned.pubkey,
        other_signed.sig,
        other_signed.unsigned.kind,
        other_signed.unsigned.created_at,
        other_signed.unsigned.content,
    );
    signer.resolve_response(&rpc_id, Ok(response_json));

    let result = poll_with_timeout(&mut op, std::time::Duration::from_secs(2));
    assert!(
        matches!(result, Err(SignerError::Mismatch(_))),
        "expected Mismatch (wrong pubkey), got {result:?}"
    );
}

#[test]
fn nip46_response_with_legitimate_created_at_skew_is_accepted() {
    // Codex review #3 explicitly noted: "the remote might have legitimately
    // massaged created_at".  The mapper trusts the response's created_at and
    // verifies the id against it.  This test pins that semantic.
    let remote_user_signer = LocalKeySigner::generate();
    let remote_user_pubkey = remote_user_signer.pubkey();

    let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com");
    let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
    let transport = Arc::new(StubTransport::default());
    let signer = handle.complete(transport.clone(), remote_user_pubkey);

    let unsigned = UnsignedEvent {
        pubkey: remote_user_pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "skewed".to_string(),
        created_at: 1_700_000_000,
    };
    let mut op = signer.sign(unsigned.clone());
    let rpc_id = transport.sent.lock().unwrap()[0].id.clone();

    // Bunker signs the same event but with a DIFFERENT created_at — and
    // ships back a self-consistent response.  Mapper accepts.
    let remote_unsigned = UnsignedEvent {
        created_at: 1_700_000_999, // skewed by 999s — legitimate clock drift
        ..unsigned
    };
    let signed = remote_user_signer
        .sign(remote_unsigned)
        .wait(std::time::Duration::from_secs(1))
        .expect("sign");
    let response_json = format!(
        r#"{{"id":"{}","pubkey":"{}","sig":"{}","kind":{},"created_at":{},"tags":[],"content":"{}"}}"#,
        signed.id,
        signed.unsigned.pubkey,
        signed.sig,
        signed.unsigned.kind,
        signed.unsigned.created_at,
        signed.unsigned.content,
    );
    signer.resolve_response(&rpc_id, Ok(response_json));

    let got = poll_with_timeout(&mut op, std::time::Duration::from_secs(2))
        .expect("legitimate skew must be accepted");
    assert_eq!(got.unsigned.created_at, 1_700_000_999);
    assert_eq!(got.id, signed.id);
}

// ============================================================================
// Fix #3 — AccountManager::remove(active) atomic rewire + idempotency
// ============================================================================

/// Records every observer event in insertion order.
#[derive(Debug, Default)]
struct RecordingObserver {
    events: Mutex<Vec<ActiveChangeEvent>>,
}

impl ActiveChangeObserver for RecordingObserver {
    fn on_active_change(&self, event: &ActiveChangeEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

#[test]
fn remove_active_account_clears_state_atomically() {
    let mut mgr = AccountManager::new()
        .with_post_condition_timeout(std::time::Duration::from_millis(500));
    let rewire = Arc::new(Kind3RewireObserver::new());
    let recorder = Arc::new(RecordingObserver::default());
    mgr.observe(rewire.clone());
    mgr.observe(recorder.clone());

    let id_a = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();
    let id_b = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();

    // Activate A, then remove A — expect: active cleared, rewire fires once
    // with current=None, recorder shows the synthetic None event.
    mgr.switch_active(&id_a).unwrap();
    assert_eq!(rewire.pending_count(), 1, "switch fires once");
    let _ = rewire.drain();
    recorder.events.lock().unwrap().clear();

    mgr.remove(&id_a).unwrap();

    // (a) active_pubkey cleared.
    assert!(mgr.active().is_none(), "active slot cleared");
    assert!(mgr.signer_active().is_none(), "signer_active is None");

    // (b) kind:3 rewire subscription teardown signal fired.
    let rewire_events = rewire.drain();
    assert_eq!(
        rewire_events.len(),
        1,
        "exactly one rewire event on active removal"
    );
    assert_eq!(rewire_events[0].previous.as_deref(), Some(id_a.as_str()));
    assert_eq!(
        rewire_events[0].current, None,
        "rewire current=None signals 'tear down active-account subs'"
    );

    // (c) AppUpdate::FullState (active_account = None) equivalent: the
    // ActiveChangeEvent with current=None is what the kernel translates into
    // the FFI FullState emission.
    let recorded = recorder.events.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1, "one observer event on active removal");
    assert_eq!(recorded[0].previous.as_deref(), Some(id_a.as_str()));
    assert!(recorded[0].current.is_none());
    assert!(recorded[0].current_pubkey.is_none());

    // (d) Idempotent — calling remove on already-removed account is a no-op
    // (no observer fires, no error returned).
    rewire.drain();
    recorder.events.lock().unwrap().clear();
    mgr.remove(&id_a)
        .expect("idempotent remove must not error on already-removed account");
    assert_eq!(
        rewire.pending_count(),
        0,
        "no rewire event on idempotent re-remove"
    );
    assert_eq!(
        recorder.events.lock().unwrap().len(),
        0,
        "no observer fire on idempotent re-remove"
    );

    // Sanity: the other account still works as expected.
    mgr.switch_active(&id_b).unwrap();
    assert_eq!(mgr.active().as_deref(), Some(id_b.as_str()));
}

#[test]
fn remove_non_active_account_does_not_fire_observer() {
    // Adjacent invariant: removing a NON-active account never fires
    // active-change observers (the active signer didn't change).
    let mut mgr = AccountManager::new()
        .with_post_condition_timeout(std::time::Duration::from_millis(500));
    let rewire = Arc::new(Kind3RewireObserver::new());
    mgr.observe(rewire.clone());

    let id_a = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();
    let id_b = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();
    mgr.switch_active(&id_a).unwrap();
    rewire.drain();

    mgr.remove(&id_b).unwrap();

    assert_eq!(
        rewire.pending_count(),
        0,
        "removing non-active account fires no rewire event"
    );
    assert_eq!(mgr.active().as_deref(), Some(id_a.as_str()), "A still active");
}

// ============================================================================
// Helpers
// ============================================================================

fn poll_with_timeout<T: Send + 'static>(
    op: &mut nmp_signers::SignerOp<T>,
    timeout: std::time::Duration,
) -> Result<T, SignerError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(r) = op.poll() {
            return r;
        }
        if std::time::Instant::now() >= deadline {
            return Err(SignerError::Timeout("poll deadline".to_string()));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
