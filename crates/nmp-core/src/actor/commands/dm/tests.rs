use super::*;
use crate::actor::commands::identity::sign_in_nsec;
use crate::actor::commands::new_bunker_handshake_slot;
use crate::actor::ActorCommand;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
const RECIPIENT: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(new_bunker_handshake_slot()),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

/// A kind:14 rumor with a `created_at: 0` sentinel — what
/// `nmp_nip17::build_dm_rumor` produces.
fn sample_rumor(sender_pubkey: &str) -> UnsignedEvent {
    UnsignedEvent {
        pubkey: sender_pubkey.to_string(),
        kind: 14,
        tags: vec![vec!["p".to_string(), RECIPIENT.to_string()]],
        content: "hello over NIP-17".to_string(),
        created_at: 0,
    }
}

#[test]
fn send_gift_wrapped_dm_without_account_toasts_and_emits_nothing() {
    let (identity, mut kernel) = fresh();
    let rumor =
        sample_rumor("aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee");
    let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, RECIPIENT, None);
    assert!(
        outbound.is_empty(),
        "no active account → no envelopes published"
    );
    assert!(
        kernel.last_error_toast_snapshot().is_some(),
        "D6: the failure is surfaced as a toast, never silent"
    );
}

#[test]
fn send_gift_wrapped_dm_rejects_malformed_recipient_pubkey() {
    let (mut identity, mut kernel) = fresh();
    sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
    let sender = identity.active_pubkey().expect("signed in");
    let rumor = sample_rumor(&sender);
    let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, "not-a-pubkey", None);
    assert!(
        outbound.is_empty(),
        "malformed recipient → nothing published"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .map(|t| t.contains("recipient pubkey"))
            .unwrap_or(false),
        "D6: malformed recipient pubkey is surfaced as a toast"
    );
}

#[test]
fn send_gift_wrapped_dm_with_local_key_gift_wraps_recipient_and_self() {
    // With a local nsec the handler must seal+wrap the rumor twice (one
    // envelope per recipient, one self-copy) and publish both — no toast.
    let (mut identity, mut kernel) = fresh();
    sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
    let sender = identity.active_pubkey().expect("signed in");

    // NIP-59 gift-wrap performs a NIP-44 ECDH against the recipient key, so
    // the recipient pubkey MUST be a real secp256k1 curve point. Derive one
    // from a freshly generated keypair rather than a hand-typed hex string.
    let recipient_pk = nostr::Keys::generate().public_key().to_hex();
    kernel.seed_kind10050_for_test(&sender, &["wss://sender-dm.relay"]);
    kernel.seed_kind10050_for_test(&recipient_pk, &["wss://recipient-dm.relay"]);

    let rumor = sample_rumor(&sender);
    let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk, None);

    assert!(
        kernel.last_error_toast_snapshot().is_none(),
        "a local-key gift-wrap send must not toast an error: {:?}",
        kernel.last_error_toast_snapshot()
    );
    let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            "wss://recipient-dm.relay".to_string(),
            "wss://sender-dm.relay".to_string(),
        ],
        "recipient and self-copy envelopes must use kind:10050 relays"
    );
}

#[test]
fn send_gift_wrapped_dm_without_recipient_kind10050_fails_closed() {
    for empty_kind10050 in [false, true] {
        let (mut identity, mut kernel) = fresh();
        sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
        let sender = identity.active_pubkey().expect("signed in");
        let recipient_pk = nostr::Keys::generate().public_key().to_hex();
        kernel.seed_kind10050_for_test(&sender, &["wss://sender-dm.relay"]);
        if empty_kind10050 {
            let id = format!("{:064x}", 0x1050);
            let _ = kernel.inject_replaceable_event(
                &id,
                &recipient_pk,
                1_000,
                10050,
                Vec::new(),
                "wss://seed",
                1_700_000_000_000,
            );
            assert!(kernel.recipient_dm_relays(&recipient_pk).is_none());
        }

        let content_relays = kernel.bootstrap_urls_for_role(crate::relay::RelayRole::Content);
        let rumor = sample_rumor(&sender);
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk, None);

        assert!(outbound.is_empty(), "missing/empty kind:10050 must not publish");
        assert!(outbound.iter().all(|m| !content_relays.contains(&m.relay_url)));
        assert!(
            kernel
                .last_error_toast_snapshot()
                .is_some_and(|t| t.contains("kind:10050")),
            "fail-closed send must surface a kind:10050 readiness error"
        );
    }
}

#[test]
fn send_gift_wrapped_dm_routes_recipient_envelope_to_kind10050_relays() {
    // NIP-17 § 2 routing: when the recipient HAS published a kind:10050
    // DM-relay list, the recipient envelope must be pinned to that list —
    // not the sender's Content relays. This is the close of the
    // inert-seam-with-warn bug: `recipient_dm_relays` now reads a live
    // kind:10050 cache, so the recipient envelope routes correctly.
    let (mut identity, mut kernel) = fresh();
    sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
    let sender = identity.active_pubkey().expect("signed in");

    // The recipient publishes a kind:10050 DM-relay list. The URL is given
    // in canonical form (no empty-path trailing slash) so the seeded value
    // and the resolved `recipient_dm_relays` value compare exactly.
    let recipient_keys = nostr::Keys::generate();
    let recipient_pk = recipient_keys.public_key().to_hex();
    kernel.seed_kind10050_for_test(&sender, &["wss://sender-dm.relay"]);
    kernel.seed_kind10050_for_test(&recipient_pk, &["wss://recipient-dm.relay"]);

    let rumor = sample_rumor(&sender);
    let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk, None);

    assert!(
        kernel.last_error_toast_snapshot().is_none(),
        "a kind:10050-routed send must not toast: {:?}",
        kernel.last_error_toast_snapshot()
    );
    // The recipient envelope must have been published to the recipient's
    // kind:10050 DM-relay list.
    assert!(
        outbound
            .iter()
            .any(|m| m.relay_url == "wss://recipient-dm.relay"),
        "the recipient envelope must route to the recipient's kind:10050 \
         DM-relay list; got: {:?}",
        outbound.iter().map(|m| &m.relay_url).collect::<Vec<_>>()
    );
}

// ── D10 fail-closed coverage ─────────────────────────────────────────
//
// The fail-closed gate (`required_dm_relays`) is exercised end-to-end by
// `send_gift_wrapped_dm_without_recipient_kind10050_fails_closed` above,
// which covers BOTH branches of the kind:10050 cache miss:
//   1. The receiver has never published a kind:10050 — `dm_relay_lists`
//      has no entry, `recipient_dm_relays` returns `None`, the helper
//      converts that to `DmRelayNotReady` and the send aborts with a toast.
//   2. The receiver published an EMPTY kind:10050 — `ingest_dm_relay_list`
//      removes the cached entry (outbox.rs guards `relays.is_empty()` →
//      `None`), so the path is structurally identical to case (1).
// Both branches are pinned in one parameterised test (the `for empty_kind10050`
// loop), so the predicate-shape micro-tests the previous design needed are
// subsumed: there is no relay-shape predicate to assert against because
// the gate happens before any envelope is constructed.

#[test]
fn send_gift_wrapped_dm_variant_is_matched_in_dispatch() {
    // Compile-time guard: the `ActorCommand::SendGiftWrappedDm` variant
    // exists with the documented shape and constructs cleanly. The actual
    // dispatch arm is exercised end-to-end by the actor loop tests; this
    // pins the variant signature so a rename (or a missing
    // `correlation_id` field, which would silently break the
    // dispatch-action spinner round-trip) breaks the build here.
    let cmd = ActorCommand::SendGiftWrappedDm {
        rumor: sample_rumor("aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee"),
        recipient_pubkey: RECIPIENT.to_string(),
        correlation_id: Some("cid-abc".to_string()),
    };
    match cmd {
        ActorCommand::SendGiftWrappedDm {
            rumor,
            recipient_pubkey,
            correlation_id,
        } => {
            assert_eq!(rumor.kind, 14, "the carried rumor is a kind:14");
            assert_eq!(recipient_pubkey, RECIPIENT);
            assert_eq!(
                correlation_id.as_deref(),
                Some("cid-abc"),
                "the variant carries the dispatched correlation_id so the \
                 actor can record `Requested` and the publish engine can \
                 report the terminal verdict against it"
            );
        }
        _ => panic!("expected SendGiftWrappedDm variant"),
    }
}
