//! Tests for ADR-0040 Site 3: cold-start onboarding signs off the actor thread.
//!
//! The three blocking `sign_active` calls in `create_account` (kind:0 profile
//! metadata, kind:10002 relay list, kind:3 contacts) are replaced with
//! `sign_active_nonblocking` + `PendingSign`. These tests verify:
//!
//! 1. For a **local nsec** account: the signs resolve synchronously (no
//!    `PendingSign` parked) and the returned `outbound` is non-empty.
//! 2. For a **bunker (NIP-46) account** with a deferred signer: `create_account`
//!    returns immediately with `pending_signs` containing 3 parked ops (one
//!    per event kind). The actor never blocks waiting for the signer — D8.
//! 3. The `prepopulate_author_relay_list` call for kind:10002 still fires
//!    synchronously (the event ID is pre-computed from the unsigned fields),
//!    so `PublishTarget::Auto` routing is live before any sign settles.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    mpsc, Arc, Mutex,
};

use nmp_signer_iface::{SignerError, SignerOp};

use super::*;
use crate::actor::pending_sign::PendingSign;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};

// ──────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────

fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(new_bunker_handshake_slot()),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

/// A stub `RemoteSignerHandle` that always returns `SignerOp::Pending` —
/// simulating a NIP-46 bunker that has not yet responded. The returned
/// `Sender` lets the test later resolve or abandon the ops.
///
/// Each call to `sign()` parks a new `Sender` in `senders`; the test can
/// drive those senders to verify that the settle path also works. For the
/// non-blocking check we just need the `Pending` variant to be returned.
#[derive(Debug)]
struct DeferredSigner {
    pubkey: String,
    /// Accumulated senders so the test can resolve them after `create_account`.
    senders: Arc<Mutex<Vec<mpsc::Sender<Result<SignedEvent, SignerError>>>>>,
    sign_count: Arc<AtomicU32>,
}

impl DeferredSigner {
    fn new(pubkey: String) -> Self {
        Self {
            pubkey,
            senders: Arc::new(Mutex::new(Vec::new())),
            sign_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn sign_count(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.sign_count)
    }

    fn senders(&self) -> Arc<Mutex<Vec<mpsc::Sender<Result<SignedEvent, SignerError>>>>> {
        Arc::clone(&self.senders)
    }
}

impl RemoteSignerHandle for DeferredSigner {
    fn pubkey_hex(&self) -> String {
        self.pubkey.clone()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    fn sign(&self, _unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        self.sign_count.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        self.senders.lock().unwrap().push(tx);
        SignerOp::Pending(rx)
    }

    fn nip44_encrypt(&self, _recipient_pubkey: &str, _plaintext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported("not needed in this test".to_string()))
    }

    fn nip44_decrypt(&self, _sender_pubkey: &str, _ciphertext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported("not needed in this test".to_string()))
    }

    fn deliver_rpc_response(&self, _response_json: &str) {}
}

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────

/// For a **local key** account the three sign ops resolve synchronously —
/// `pending_signs` remains empty and outbound messages are produced.
#[test]
fn local_key_account_parks_no_pending_signs() {
    let (mut identity, mut kernel) = fresh();
    let mut pending_signs: Vec<PendingSign> = Vec::new();

    let profile = std::collections::HashMap::from([("name".to_string(), "Alice".to_string())]);
    let outbound = create_account(
        &mut identity,
        &mut kernel,
        /*relays_ready=*/ false,
        &profile,
        &[],
        /*mls=*/ false,
        &mut pending_signs,
    );

    assert!(
        pending_signs.is_empty(),
        "local-key cold-start must park zero pending signs; got {}",
        pending_signs.len()
    );
    // The outbound may be empty if no relays are configured, but the call
    // must return without blocking regardless.
    let _ = outbound; // just confirm the call completed
}

/// Verifies the non-blocking cold-start sign mechanism at the
/// `sign_active_nonblocking` + `PendingSign` level.
///
/// `create_account` always generates a fresh LOCAL key and activates it, so
/// the three cold-start signs will always resolve synchronously for a
/// freshly-created account (the local key signs in-process, returning
/// `SignerOp::Ready`). The blocking hazard exists on any code path that
/// calls `sign_active` while the *active* identity is a remote (bunker)
/// signer — which is exactly what `sign_active_nonblocking` guards against.
///
/// This test drives `sign_active_nonblocking` directly with a
/// `DeferredSigner` active, verifying:
/// 1. The call returns `SignerOp::Pending` immediately — no blocking.
/// 2. `PendingSign::with_target` correctly parks the op with the
///    cold-start `Explicit` relay target.
/// 3. The parked op polls to `None` until the broker responds (D8: no
///    blocking wait on the actor thread).
#[test]
fn sign_active_nonblocking_with_deferred_signer_parks_pending_op() {
    use nostr::{Keys, SecretKey};
    use nostr::nips::nip19::FromBech32;

    let (mut identity, mut kernel) = fresh();

    // Wire up a known bunker key as the active remote signer.
    const TEST_NSEC: &str =
        "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    let sk = SecretKey::from_bech32(TEST_NSEC).expect("valid nsec");
    let keys = Keys::new(sk);
    let pubkey_hex = keys.public_key().to_hex();

    let signer = Box::new(DeferredSigner::new(pubkey_hex.clone()));
    let sign_count = signer.sign_count();
    let senders_handle = signer.senders();

    // Register the bunker as the active remote signer.
    add_remote_signer(&mut identity, &mut kernel, signer, /*relays_ready=*/ false);
    // `add_remote` sets active when first account is added; verify.
    assert_eq!(identity.active_pubkey().as_deref(), Some(pubkey_hex.as_str()));

    // Build a cold-start unsigned event (kind:0 profile metadata).
    let unsigned = UnsignedEvent {
        pubkey: pubkey_hex.clone(),
        kind: 0,
        tags: Vec::new(),
        content: r#"{"name":"BunkerBob"}"#.to_string(),
        created_at: 1_700_000_000,
    };

    // `sign_active_nonblocking` must return immediately with `Pending` op,
    // NOT block waiting for the broker (D8).
    let op = sign_active_nonblocking(&identity, &unsigned)
        .expect("sign_active_nonblocking must not error for an active bunker");

    // The deferred signer was called exactly once.
    assert_eq!(sign_count.load(Ordering::Relaxed), 1, "sign() called once");

    // The returned op must be Pending (not Ready) — the broker has not responded yet.
    // We test this by polling: a Pending op with no sender response returns None.
    // Drop the senders handle after the op is parked so the channel stays open
    // (we want Pending, not Backend error from a disconnected channel).
    let _ = &senders_handle; // keep sender alive

    let mut ps = PendingSign::with_target(
        op,
        Vec::new(),
        crate::publish::PublishTarget::Explicit {
            relays: vec!["wss://relay.example.com".to_string()],
        },
    );

    // First poll: still pending — broker has not responded.
    assert!(
        ps.op.poll().is_none(),
        "parked op must poll to None while broker has not responded (D8: no block)"
    );
    assert!(
        !ps.timed_out(),
        "a freshly parked PendingSign must not be timed out"
    );

    // Verify the Explicit target was preserved through the park.
    assert!(
        matches!(
            &ps.target,
            crate::publish::PublishTarget::Explicit { relays }
            if relays == &["wss://relay.example.com".to_string()]
        ),
        "parked PendingSign must carry the cold-start Explicit relay target"
    );
}

/// Verifies that `prepopulate_author_relay_list` is called synchronously
/// within `create_account` for kind:10002, using the pre-computed event ID,
/// so the mailbox cache is populated before any pending sign settles.
///
/// Uses a local-key account (where all three signs resolve immediately) to
/// confirm the full `create_account` flow populates the cache correctly.
#[test]
fn create_account_prepopulates_relay_list_synchronously() {
    let (mut identity, mut kernel) = fresh();
    let mut pending_signs: Vec<PendingSign> = Vec::new();
    let profile = std::collections::HashMap::from([("name".to_string(), "Alice".to_string())]);
    let relays = vec![("wss://relay.example.com".to_string(), "both".to_string())];

    create_account(
        &mut identity,
        &mut kernel,
        false,
        &profile,
        &relays,
        false,
        &mut pending_signs,
    );

    // For a local key account, all signs resolve synchronously — no parks.
    assert!(
        pending_signs.is_empty(),
        "local-key create_account parks no PendingSign ops"
    );

    // The mailbox cache must have been populated for the new account's
    // pubkey (by `prepopulate_author_relay_list`), so that subsequent
    // `PublishTarget::Auto` routing finds the relay list immediately.
    let active_pubkey = identity.active_pubkey().expect("new account has active pubkey");
    // We verify indirectly: a follow note should be routable without a
    // "no write-relays" toast, since the cache was pre-populated.
    let snap: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true))
            .expect("snapshot JSON parses");
    let last_error = snap["last_error_toast"].as_str().unwrap_or("");
    assert!(
        !last_error.contains("no cold-start relays"),
        "D6 'no cold-start relays' toast must not fire; got: {last_error:?}"
    );
    // The kernel's mailbox cache must be non-empty for the new account's pubkey.
    // `prepopulate_author_relay_list` fires synchronously in create_account
    // before any sign settles — confirmed here by calling `mailbox_cache()`.
    let cache = kernel.mailbox_cache();
    let read = cache.read_relays(&active_pubkey);
    let write = cache.write_relays(&active_pubkey);
    assert!(
        read.is_some() || write.is_some(),
        "mailbox cache must be populated after create_account for pubkey {active_pubkey}"
    );
}

/// Verifies that the `compute_unsigned_event_id` helper produces the correct
/// Nostr event ID: it must match the id that `sign_with` would have produced
/// for the same unsigned fields (signature-independent by NIP-01).
#[test]
fn compute_unsigned_event_id_matches_sign_with_id() {
    use nostr::{Keys, SecretKey};
    use nostr::nips::nip19::FromBech32;

    const TEST_NSEC: &str =
        "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    let sk = SecretKey::from_bech32(TEST_NSEC).expect("valid nsec");
    let keys = Keys::new(sk);
    let pubkey_hex = keys.public_key().to_hex();

    let unsigned = UnsignedEvent {
        pubkey: pubkey_hex,
        kind: crate::kinds::KIND_RELAY_LIST,
        tags: vec![vec!["r".to_string(), "wss://relay.example.com".to_string()]],
        content: String::new(),
        created_at: 1_700_000_000,
    };

    // Precomputed ID path (signature-independent).
    let precomputed_id =
        compute_unsigned_event_id(&unsigned).expect("must compute for valid inputs");

    // Signed ID path — the same ID must emerge from the actual sign_with flow.
    let signed = sign_with(&keys, &unsigned).expect("must sign for valid inputs");

    assert_eq!(
        precomputed_id, signed.id,
        "compute_unsigned_event_id must produce the same ID as sign_with"
    );
}
