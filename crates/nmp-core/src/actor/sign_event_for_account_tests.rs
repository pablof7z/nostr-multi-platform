//! ADR-0043 Decision 2 — `ActorCommand::SignEventForAccount` dispatch-arm +
//! idle-loop drain tests for BOTH signing backends.
//!
//! These prove the worker code path is identical for a local nsec and a
//! NIP-46 bunker:
//!
//! 1. **Local (inline)** — a local-key account resolves `SignerOp::Ready` on
//!    the spot, so the dispatch arm invokes the continuation INLINE on the
//!    actor thread with a valid `SignedEvent` (id + sig + pubkey verified). No
//!    op is parked.
//! 2. **Mock bunker (parked → resolved)** — a remote signer returns
//!    `SignerOp::Pending`; the dispatch arm parks a `ParkedOp` with the
//!    `SignContinuation` sink. The continuation has NOT run yet. After the broker
//!    turns the request around, the idle-loop drain
//!    (`resolve_parked_op`) invokes the SAME continuation with the
//!    `SignedEvent`.
//! 3. **Mock bunker error** — a broker rejection / dropped channel resolves the
//!    continuation with `Err(_)` so the worker's failure path runs (D6 — no
//!    stuck spinner).
//!
//! The continuation in all three is a worker-supplied closure that records the
//! outcome through a shared `Arc<Mutex<..>>` (the Blossom-worker shape, no HTTP).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use nmp_signer_iface::{SignerError, SignerOp};
use nostr::nips::nip19::FromBech32;
use nostr::{EventBuilder, Keys, SecretKey, Timestamp};

use super::commands::{self, IdentityRuntime};
use super::pending_sign::{resolve_parked_op, ParkedOpSink};
use super::signer_port_test_harness::dispatch_one;
use super::{ActorCommand, SignContinuation};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};

/// Known-good test nsec (shared with `remote_signer_tests`).
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

fn test_keys() -> Keys {
    Keys::new(SecretKey::from_bech32(TEST_NSEC).expect("valid nsec"))
}

/// An unsigned kind:24242-shaped draft (any kind works; this mirrors a Blossom
/// auth event). `created_at` is already stamped (the dispatch arm does not
/// re-stamp — the caller owns D7 before constructing the command).
fn draft_unsigned(pubkey_hint: &str) -> UnsignedEvent {
    UnsignedEvent {
        pubkey: pubkey_hint.to_string(),
        kind: 24242,
        tags: vec![
            vec!["t".to_string(), "upload".to_string()],
            vec!["x".to_string(), "ab".repeat(32)],
            vec!["expiration".to_string(), "1700000300".to_string()],
        ],
        content: "Upload blob".to_string(),
        created_at: 1_700_000_000,
    }
}

/// A remote-signer stub whose `sign` returns `SignerOp::Pending` — the broker
/// round-trip is driven by the test through the returned [`Sender`]. This is
/// the bunker shape the dispatch arm must park (the existing
/// `remote_signer_tests::StubRemoteSigner` resolves `Ready`, so the park path
/// is never exercised there).
#[derive(Debug)]
struct PendingRemoteSigner {
    keys: Keys,
    pk: String,
    sign_count: Arc<AtomicU32>,
    /// Self-described per-op budget (ADR-0050 §D4). Defaults to the NIP-46 5s
    /// budget; the named-roster-key deadline test overrides it to a 90s
    /// NIP-55-style budget to prove the parked deadline reflects the SIGNING
    /// account's budget rather than the active account's.
    op_timeout: std::time::Duration,
    /// Each `sign()` stashes the receiver end here so the dispatch arm can park
    /// it; the test holds the matching sender to resolve the broker round-trip.
    last_sender: Mutex<Option<Sender<Result<SignedEvent, SignerError>>>>,
}

impl PendingRemoteSigner {
    fn new(keys: Keys) -> Self {
        Self::with_op_timeout(keys, nmp_signer_iface::PENDING_SIGN_TIMEOUT)
    }

    fn with_op_timeout(keys: Keys, op_timeout: std::time::Duration) -> Self {
        let pk = keys.public_key().to_hex();
        Self {
            keys,
            pk,
            sign_count: Arc::new(AtomicU32::new(0)),
            op_timeout,
            last_sender: Mutex::new(None),
        }
    }

    /// Build the `SignedEvent` the broker would return for `unsigned`.
    fn signed_for(&self, unsigned: &UnsignedEvent) -> SignedEvent {
        let kind = nostr::Kind::from_u16(unsigned.kind as u16);
        let tags = unsigned
            .tags
            .iter()
            .filter_map(|t| nostr::Tag::parse(t).ok())
            .collect::<Vec<_>>();
        let event = EventBuilder::new(kind, &unsigned.content)
            .tags(tags)
            .custom_created_at(Timestamp::from(unsigned.created_at))
            .sign_with_keys(&self.keys)
            .expect("stub sign");
        SignedEvent {
            id: event.id.to_hex(),
            sig: event.sig.to_string(),
            unsigned: UnsignedEvent {
                pubkey: event.pubkey.to_hex(),
                kind: event.kind.as_u16() as u32,
                tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: event.content.clone(),
                created_at: event.created_at.as_secs(),
            },
        }
    }
}

impl RemoteSignerHandle for PendingRemoteSigner {
    fn pubkey_hex(&self) -> String {
        self.pk.clone()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    fn op_timeout(&self) -> std::time::Duration {
        self.op_timeout
    }

    fn sign(&self, _unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        self.sign_count.fetch_add(1, Ordering::Relaxed);
        let (tx, rx): (
            Sender<Result<SignedEvent, SignerError>>,
            Receiver<Result<SignedEvent, SignerError>>,
        ) = channel();
        *self.last_sender.lock().unwrap() = Some(tx);
        SignerOp::Pending(rx)
    }

    fn nip44_encrypt(&self, _recipient_pubkey: &str, _plaintext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Backend("unused".into()))
    }

    fn nip44_decrypt(&self, _sender_pubkey: &str, _ciphertext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Backend("unused".into()))
    }

    fn deliver_response(&self, _response_json: &str) {}
}


fn fresh_identity() -> IdentityRuntime {
    IdentityRuntime::new(
        commands::new_bunker_handshake_slot(),
        commands::new_signer_state_slot(),
    )
}

/// Captured continuation outcome: `Some(Ok(signed))` / `Some(Err(reason))` once
/// the continuation ran, `None` while it has not.
type CapturedOutcome = Arc<Mutex<Option<Result<SignedEvent, String>>>>;

fn capture_continuation() -> (CapturedOutcome, SignContinuation) {
    let captured: CapturedOutcome = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&captured);
    let continuation = SignContinuation::new(move |outcome| {
        *slot.lock().unwrap() = Some(outcome);
    });
    (captured, continuation)
}

// ─────────────────────────────────────────────────────────────────────────────
// Backend 1 — local nsec resolves Ready: continuation runs INLINE.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn local_backend_invokes_continuation_inline_with_valid_signed_event() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Sign in a LOCAL nsec account (make_active = true).
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let active_pk = identity.active_pubkey().expect("active account");

    let (captured, continuation) = capture_continuation();
    let unsigned = draft_unsigned(&active_pk);

    let parked = dispatch_one(
        ActorCommand::SignEventForAccount {
            unsigned: unsigned.clone(),
            signer_pubkey: None, // active account
            continuation,
        },
        &mut identity,
        &mut kernel,
    );

    // A local key resolves Ready — nothing is parked, the continuation ran
    // INLINE on the dispatch (actor) thread.
    assert!(
        parked.is_empty(),
        "local key resolves Ready — no PendingSignReturn should be parked"
    );
    let outcome = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation must run inline for a local key");
    let signed = outcome.expect("local sign must succeed");

    // The signed event is valid and bound to the active account.
    assert_eq!(signed.unsigned.kind, 24242);
    assert_eq!(signed.unsigned.content, "Upload blob");
    assert_eq!(
        signed.unsigned.pubkey, active_pk,
        "signed event pubkey must be the active account"
    );
    assert_eq!(signed.id.len(), 64, "id is 32-byte hex");
    assert_eq!(signed.sig.len(), 128, "sig is 64-byte hex");

    // Verify the signature actually validates against the public key (not a
    // vacuous shape check) — round-trip through nostr::Event.
    let event_json = crate::actor::dispatch::signed_event_to_json(&signed);
    let event: nostr::Event = serde_json::from_str(&event_json).expect("flat NIP-01 JSON");
    assert!(event.verify().is_ok(), "signature must verify");
    assert_eq!(event.pubkey.to_hex(), active_pk);
}

/// `signer_pubkey: Some(pk)` routes through `sign_with_account_nonblocking`
/// (the named-roster-key path Blossom uses for per-podcast keys). Sign with the
/// active local account named explicitly by pubkey — the continuation still
/// runs inline with a valid, verifiable signature.
#[test]
fn local_backend_named_pubkey_signs_with_account() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let pk = identity.active_pubkey().expect("active account");

    let (captured, continuation) = capture_continuation();
    let parked = dispatch_one(
        ActorCommand::SignEventForAccount {
            unsigned: draft_unsigned(&pk),
            signer_pubkey: Some(pk.clone()), // NAMED roster key, not active-default
            continuation,
        },
        &mut identity,
        &mut kernel,
    );
    assert!(parked.is_empty(), "named local key resolves Ready inline");
    let signed = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation ran inline")
        .expect("named-account sign succeeds");
    assert_eq!(signed.unsigned.pubkey, pk);
    let event_json = crate::actor::dispatch::signed_event_to_json(&signed);
    let event: nostr::Event = serde_json::from_str(&event_json).expect("flat NIP-01");
    assert!(
        event.verify().is_ok(),
        "named-account signature must verify"
    );
}

/// `signer_pubkey: Some(unknown)` — no signer for the named pubkey — resolves
/// the continuation with `Err` immediately (D6).
#[test]
fn named_pubkey_with_no_signer_invokes_continuation_with_err() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let unknown = "deadbeef".repeat(8);

    let (captured, continuation) = capture_continuation();
    let parked = dispatch_one(
        ActorCommand::SignEventForAccount {
            unsigned: draft_unsigned(&unknown),
            signer_pubkey: Some(unknown),
            continuation,
        },
        &mut identity,
        &mut kernel,
    );
    assert!(parked.is_empty(), "no signer → nothing parked");
    let outcome = captured.lock().unwrap().take().expect("continuation ran");
    assert!(
        outcome.is_err(),
        "an unknown named account is an Err outcome"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Backend 2 — NIP-46 bunker resolves Pending: park → drain → continuation runs.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn bunker_backend_parks_then_drain_invokes_continuation_with_signed_event() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Register a Pending remote signer as the active account. Keep an Arc to
    // its sign-count + the keys so we can mint the broker's eventual response.
    let keys = test_keys();
    let stub = PendingRemoteSigner::new(keys.clone());
    let sign_count = Arc::clone(&stub.sign_count);
    let stub_pk = stub.pubkey_hex();
    // We need the stub AFTER add_signer consumes it (to read `last_sender`), so
    // build the signed response up front from a parallel signer with the same
    // keys.
    let responder = PendingRemoteSigner::new(keys);

    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(stub)),
        true,
        false,
    );
    assert_eq!(identity.active_pubkey().as_deref(), Some(stub_pk.as_str()));

    let (captured, continuation) = capture_continuation();
    let unsigned = draft_unsigned(&stub_pk);

    let mut parked = dispatch_one(
        ActorCommand::SignEventForAccount {
            unsigned: unsigned.clone(),
            signer_pubkey: None,
            continuation,
        },
        &mut identity,
        &mut kernel,
    );

    // Bunker resolves Pending — the op is PARKED and the continuation has NOT
    // run yet (this is the asynchronous broker round-trip).
    assert_eq!(
        sign_count.load(Ordering::Relaxed),
        1,
        "stub signed exactly once"
    );
    assert_eq!(parked.len(), 1, "a Pending bunker op must be parked");
    assert!(
        captured.lock().unwrap().is_none(),
        "continuation must NOT run while the broker round-trip is pending"
    );

    // The broker turns the request around. Pull the receiver the parked op is
    // polling and feed it the signed event.
    let signed = responder.signed_for(&unsigned);
    {
        // Replace the parked op (inside the SignContinuation sink) with one whose
        // sender we control, then resolve it — mirrors a later idle-tick delivery.
        let ParkedOpSink::SignContinuation { op, .. } = &mut parked[0].sink else {
            panic!("expected a SignContinuation sink");
        };
        let (tx, rx): (
            Sender<Result<SignedEvent, SignerError>>,
            Receiver<Result<SignedEvent, SignerError>>,
        ) = channel();
        *op = SignerOp::Pending(rx);
        tx.send(Ok(signed.clone())).unwrap();
    }

    // First drain tick resolves it: the SAME continuation runs, now from the
    // idle-loop drain (not inline) — the worker code path is identical.
    let drained = resolve_parked_op(&mut parked[0], &mut kernel);
    assert!(!drained.keep, "a resolved op is dropped from the parked queue");

    let outcome = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation must run from the drain once the broker responds");
    let got = outcome.expect("bunker sign must succeed");
    assert_eq!(got.unsigned.kind, 24242);
    assert_eq!(got.unsigned.content, "Upload blob");
    assert_eq!(got.unsigned.pubkey, stub_pk);

    // Same signature-verification rigour as the local path.
    let event_json = crate::actor::dispatch::signed_event_to_json(&got);
    let event: nostr::Event = serde_json::from_str(&event_json).expect("flat NIP-01 JSON");
    assert!(event.verify().is_ok(), "bunker signature must verify");
}

// ─────────────────────────────────────────────────────────────────────────────
// Backend 2 (error) — broker rejection resolves the continuation with Err.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn bunker_backend_error_invokes_continuation_with_err_so_terminal_resolves() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = test_keys();
    let stub = PendingRemoteSigner::new(keys);
    let stub_pk = stub.pubkey_hex();
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(stub)),
        true,
        false,
    );

    let (captured, continuation) = capture_continuation();
    let mut parked = dispatch_one(
        ActorCommand::SignEventForAccount {
            unsigned: draft_unsigned(&stub_pk),
            signer_pubkey: None,
            continuation,
        },
        &mut identity,
        &mut kernel,
    );
    assert_eq!(parked.len(), 1, "Pending op parked");

    // Broker rejects the sign request.
    {
        let ParkedOpSink::SignContinuation { op, .. } = &mut parked[0].sink else {
            panic!("expected SignContinuation sink");
        };
        let (tx, rx) = channel::<Result<SignedEvent, SignerError>>();
        *op = SignerOp::Pending(rx);
        tx.send(Err(SignerError::Rejected("user declined".to_string())))
            .unwrap();
    }
    let drained = resolve_parked_op(&mut parked[0], &mut kernel);
    assert!(!drained.keep, "a rejected op is dropped");

    let outcome = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation must run on broker rejection (D6 — no stuck spinner)");
    let reason = outcome.expect_err("a rejection is an Err outcome");
    assert!(
        reason.contains("declined") || reason.to_lowercase().contains("reject"),
        "error reason should surface the broker rejection: {reason}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// No active account — continuation resolves Err immediately (D6).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn no_account_invokes_continuation_with_err_immediately() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let (captured, continuation) = capture_continuation();
    let parked = dispatch_one(
        ActorCommand::SignEventForAccount {
            unsigned: draft_unsigned(""),
            signer_pubkey: None,
            continuation,
        },
        &mut identity,
        &mut kernel,
    );
    assert!(parked.is_empty(), "no account → nothing parked");
    let outcome = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation must run immediately when there is no account");
    assert!(outcome.is_err(), "no active account is an Err outcome");
}

// ─────────────────────────────────────────────────────────────────────────────
// §D4 — SignEventForReturn named-account budget regression.
//
// Mirrors `cipher_for_account_tests::named_roster_key_keeps_its_own_budget_not_
// the_active_accounts` on the sign-and-return path (`dispatch.rs:606`). A named
// 90s NIP-55-style roster key signed while a 5s NIP-46-style account is active
// must park with ITS OWN 90s budget — never inherit the active account's 5s
// deadline. The D4 bug was `active_sign_deadline()` at the park site; the fix
// computes `sign_deadline_for(named)`.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn sign_event_for_return_named_roster_key_keeps_its_own_budget() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Active account: a 5s-budget bunker (NIP-46-style). Pending sign so any
    // accidental routing-through-active would park with the 5s deadline.
    let active = PendingRemoteSigner::with_op_timeout(
        Keys::generate(),
        std::time::Duration::from_secs(5),
    );
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(active)),
        true,
        false,
    );

    // A SECOND, non-active roster key: a 90s-budget signer (NIP-55-style).
    let named = PendingRemoteSigner::with_op_timeout(
        Keys::generate(),
        std::time::Duration::from_secs(90),
    );
    let named_pk = named.pubkey_hex();
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(named)),
        false, // do NOT make active — the 5s account stays active.
        false,
    );

    // Sign-and-return with the NAMED key while the 5s account is active.
    let before = std::time::Instant::now();
    let parked = dispatch_one(
        ActorCommand::SignEventForReturn {
            account_pubkey: named_pk,
            unsigned_json: r#"{"kind":24242,"content":"auth","tags":[]}"#.to_string(),
            correlation_id: "corr-named-budget".to_string(),
        },
        &mut identity,
        &mut kernel,
    );
    assert_eq!(parked.len(), 1, "named bunker sign-and-return parks");

    // The parked deadline must reflect the NAMED key's 90s budget, NOT the
    // active account's 5s. Generous slack for dispatch latency.
    let deadline = parked[0].deadline;
    let budget = deadline.saturating_duration_since(before);
    assert!(
        budget > std::time::Duration::from_secs(60),
        "named 90s key must keep its own budget on the sign-and-return path (got \
         {budget:?}); the D4 bug would have parked it with the active account's 5s budget"
    );
}
