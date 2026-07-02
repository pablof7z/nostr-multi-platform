//! Backend 1 — local nsec resolves `SignerOp::Ready`: the continuation runs
//! INLINE on the dispatch (actor) thread, both for the active account and for
//! a named roster key. Also covers the "named pubkey has no signer" Err path
//! (D6 — no stuck spinner).

use super::support::{capture_continuation, draft_unsigned, fresh_identity, TEST_NSEC};
use crate::actor::commands;
use crate::actor::signer_port_test_harness::dispatch_one;
use crate::actor::{ActorCommand, SignCommand};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

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
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned: unsigned.clone(),
            signer_pubkey: None, // active account
            continuation,
        }),
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
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned: draft_unsigned(&pk),
            signer_pubkey: Some(pk.clone()), // NAMED roster key, not active-default
            continuation,
        }),
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
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned: draft_unsigned(&unknown),
            signer_pubkey: Some(unknown),
            continuation,
        }),
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
