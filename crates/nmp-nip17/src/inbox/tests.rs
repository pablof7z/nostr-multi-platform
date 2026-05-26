use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

/// Wrap `rumor` for `receiver` via the ADR-0026 `SignerForSeal` seam. The
/// blanket `SignerForSeal` impl on `nostr::Keys` resolves every `SignerOp`
/// synchronously, so `.wait` returns immediately without spawning a driver.
fn gift_wrap_test(
    sender: &Keys,
    receiver: &nostr::PublicKey,
    rumor: nostr::UnsignedEvent,
) -> Event {
    let signer: std::sync::Arc<dyn nmp_nip59::SignerForSeal> =
        std::sync::Arc::new(sender.clone());
    // Tests pass deterministic timestamps via the rumor; reuse it for the
    // seal/wrap envelope so the test-stable ordering survives migration.
    let created_at = rumor.created_at;
    nmp_nip59::gift_wrap_with_signer(&signer, receiver, &rumor, created_at)
        .wait(nmp_nip59::GIFT_WRAP_TOTAL_TIMEOUT)
        .expect("gift wrap succeeds")
}

/// Build a signed kind:1059 gift-wrap envelope carrying a kind:14 rumor
/// from `sender` to `receiver`, mirroring NIP-59 §2.
fn gift_wrapped_dm(
    sender: &Keys,
    receiver: &nostr::PublicKey,
    content: &str,
    created_at: u64,
    reply_to: Option<&str>,
) -> Event {
    let mut tags = vec![Tag::public_key(*receiver)];
    if let Some(parent) = reply_to {
        // NIP-10 reply marker — `["e", <id>, <relay-hint>, "reply"]`.
        tags.push(
            Tag::parse([
                "e".to_string(),
                parent.to_string(),
                String::new(),
                "reply".to_string(),
            ])
            .expect("well-formed e tag"),
        );
    }
    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(tags)
        .custom_created_at(Timestamp::from(created_at))
        .build(sender.public_key());
    gift_wrap_test(sender, receiver, rumor)
}

/// A projection bound to `keys` as the active local account.
fn inbox_for(keys: &Keys) -> DmInboxProjection {
    DmInboxProjection::new(Arc::new(Mutex::new(Some(keys.clone()))))
}

#[test]
fn fresh_inbox_yields_empty_snapshot() {
    // With no local keys, the snapshot is empty AND marks remote_signer_unsupported
    // (V-08 Stage 1: the host can distinguish "no signer" from "has DMs").
    let inbox = DmInboxProjection::new(Arc::new(Mutex::new(None)));
    let snap = inbox.snapshot();
    assert!(snap.conversations.is_empty());
    assert!(snap.remote_signer_unsupported, "no-keys slot must set the flag");
    assert_eq!(
        inbox.snapshot_json(),
        serde_json::json!({ "conversations": [], "remote_signer_unsupported": true })
    );
}

#[test]
fn kind_filter_is_gift_wrap_only() {
    let filter = DmInboxProjection::kind_filter();
    assert!(
        filter.matches(KIND_GIFT_WRAP),
        "kind:1059 gift-wrap must match"
    );
    assert!(!filter.matches(14), "kind:14 must NOT match — it is sealed");
    assert!(!filter.matches(1), "plain notes must not match");
}

#[test]
fn not_signed_in_is_silent_no_op() {
    // No local keys → every envelope is discarded, no panic.
    let inbox = DmInboxProjection::new(Arc::new(Mutex::new(None)));
    let alice = Keys::generate();
    let bob = Keys::generate();
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "hi", 100, None);
    assert!(!inbox.ingest_gift_wrap(&envelope.as_json(), None));
    assert!(inbox.snapshot().conversations.is_empty());
}

#[test]
fn malformed_json_is_silent_no_op() {
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);
    assert!(!inbox.ingest_gift_wrap("not json at all", None));
    assert!(!inbox.ingest_gift_wrap("{}", None));
    assert!(inbox.snapshot().conversations.is_empty());
}

#[test]
fn envelope_for_another_recipient_is_discarded() {
    // Alice wraps a DM to Carol; Bob's inbox cannot decrypt it.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let carol = Keys::generate();
    let inbox = inbox_for(&bob);
    let envelope = gift_wrapped_dm(&alice, &carol.public_key(), "secret", 100, None);
    assert!(
        !inbox.ingest_gift_wrap(&envelope.as_json(), None),
        "an envelope sealed for Carol must not decrypt for Bob"
    );
    assert!(inbox.snapshot().conversations.is_empty());
}

#[test]
fn received_dm_surfaces_in_the_conversation() {
    // Alice → Bob. Bob's inbox decrypts and files it under peer = Alice.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "hello bob", 12345, None);
    assert!(inbox.ingest_gift_wrap(&envelope.as_json(), None));

    let snap = inbox.snapshot();
    assert_eq!(snap.conversations.len(), 1);
    let convo = &snap.conversations[0];
    assert_eq!(
        convo.peer_pubkey,
        alice.public_key().to_hex(),
        "the conversation peer is the sender"
    );
    assert_eq!(convo.messages.len(), 1);
    let msg = &convo.messages[0];
    assert_eq!(msg.content, "hello bob");
    assert_eq!(msg.sender_pubkey, alice.public_key().to_hex());
    assert_eq!(msg.created_at, 12345, "D7: the rumor's send time verbatim");
    assert_eq!(msg.reply_to, None);
    assert!(
        !msg.is_outgoing,
        "a message sent by Alice to Bob's inbox is incoming (not outgoing)"
    );
}

#[test]
fn self_copy_files_under_the_recipient_peer() {
    // Bob sends to Alice and gift-wraps a self-copy to himself. Bob's
    // inbox decrypts the self-copy; the peer must be Alice (the `p` tag),
    // NOT Bob.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);
    // The self-copy: sender == receiver == Bob, p-tag == Alice.
    let self_copy = {
        let rumor = EventBuilder::new(Kind::from_u16(14), "sent to alice")
            .tags(vec![Tag::public_key(alice.public_key())])
            .custom_created_at(Timestamp::from(500))
            .build(bob.public_key());
        gift_wrap_test(&bob, &bob.public_key(), rumor)
    };
    assert!(inbox.ingest_gift_wrap(&self_copy.as_json(), None));

    let snap = inbox.snapshot();
    assert_eq!(snap.conversations.len(), 1);
    assert_eq!(
        snap.conversations[0].peer_pubkey,
        alice.public_key().to_hex(),
        "a self-copy files under the recipient, not the local sender"
    );
    assert_eq!(
        snap.conversations[0].messages[0].sender_pubkey,
        bob.public_key().to_hex(),
        "the message author is still Bob (the local sender)"
    );
    assert!(
        snap.conversations[0].messages[0].is_outgoing,
        "a self-copy whose seal authenticates the local key is outgoing"
    );
}

#[test]
fn sent_and_received_share_one_conversation() {
    // A full round-trip: Alice→Bob (received) and Bob→Alice self-copy
    // (sent) both land in the SAME conversation keyed on peer = Alice.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);

    let received = gift_wrapped_dm(&alice, &bob.public_key(), "hi bob", 100, None);
    inbox.ingest_gift_wrap(&received.as_json(), None);

    let sent = {
        let rumor = EventBuilder::new(Kind::from_u16(14), "hi alice")
            .tags(vec![Tag::public_key(alice.public_key())])
            .custom_created_at(Timestamp::from(200))
            .build(bob.public_key());
        gift_wrap_test(&bob, &bob.public_key(), rumor)
    };
    inbox.ingest_gift_wrap(&sent.as_json(), None);

    let snap = inbox.snapshot();
    assert_eq!(
        snap.conversations.len(),
        1,
        "sent + received with one peer is one thread"
    );
    let convo = &snap.conversations[0];
    assert_eq!(convo.messages.len(), 2);
    // Chronological ordering within the thread — oldest first, newest
    // last. "hi bob" was stamped at 100, "hi alice" at 200.
    assert_eq!(convo.messages[0].content, "hi bob");
    assert!(!convo.messages[0].is_outgoing, "Alice→Bob is incoming");
    assert_eq!(convo.messages[1].content, "hi alice");
    assert!(
        convo.messages[1].is_outgoing,
        "Bob's self-copy of his outbound DM is outgoing"
    );
}

#[test]
fn reply_marker_is_extracted() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);
    let parent_id = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccdd00";
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "replying", 300, Some(parent_id));
    assert!(inbox.ingest_gift_wrap(&envelope.as_json(), None));

    let snap = inbox.snapshot();
    assert_eq!(
        snap.conversations[0].messages[0].reply_to.as_deref(),
        Some(parent_id),
        "the NIP-10 reply e-tag must surface as reply_to"
    );
}

#[test]
fn duplicate_envelope_is_not_duplicated() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "once", 100, None);
    // Same envelope delivered twice — the inner rumor id is identical.
    inbox.ingest_gift_wrap(&envelope.as_json(), None);
    inbox.ingest_gift_wrap(&envelope.as_json(), None);
    let snap = inbox.snapshot();
    assert_eq!(snap.conversations.len(), 1);
    assert_eq!(
        snap.conversations[0].messages.len(),
        1,
        "a re-delivered envelope must not duplicate the message"
    );
}

#[test]
fn redelivered_dm_records_source_relays() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let proj = Arc::new(inbox_for(&bob));
    let observer: Arc<dyn RawEventObserver> = Arc::clone(&proj) as _;
    let envelope =
        gift_wrapped_dm(&alice, &bob.public_key(), "from relays", 100, None).as_json();

    observer.on_raw_event_with_source(KIND_GIFT_WRAP, &envelope, Some("wss://r1.example"));
    observer.on_raw_event_with_source(KIND_GIFT_WRAP, &envelope, Some("wss://r2.example"));
    observer.on_raw_event_with_source(KIND_GIFT_WRAP, &envelope, Some("wss://r1.example"));

    let snap = proj.snapshot();
    let relays = &snap.conversations[0].messages[0].source_relays;
    assert_eq!(
        relays,
        &vec![
            "wss://r1.example".to_string(),
            "wss://r2.example".to_string()
        ],
        "the DM inbox must retain deduped source relay provenance"
    );
}

#[test]
fn conversations_ordered_by_most_recent_message() {
    // Two peers; the one with the newer message must sort first.
    let alice = Keys::generate();
    let carol = Keys::generate();
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);

    inbox.ingest_gift_wrap(
        &gift_wrapped_dm(&alice, &bob.public_key(), "older", 100, None).as_json(),
        None,
    );
    inbox.ingest_gift_wrap(
        &gift_wrapped_dm(&carol, &bob.public_key(), "newer", 900, None).as_json(),
        None,
    );

    let snap = inbox.snapshot();
    assert_eq!(snap.conversations.len(), 2);
    assert_eq!(
        snap.conversations[0].peer_pubkey,
        carol.public_key().to_hex(),
        "the conversation with the newest message sorts first"
    );
}

#[test]
fn drives_through_raw_observer_trait_object() {
    // The projection must be usable as `Arc<dyn RawEventObserver>` — that
    // is exactly how a host FFI registers it.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let proj = Arc::new(inbox_for(&bob));
    let observer: Arc<dyn RawEventObserver> = Arc::clone(&proj) as _;
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "via trait", 100, None);
    observer.on_raw_event(KIND_GIFT_WRAP, &envelope.as_json());
    assert_eq!(proj.snapshot().conversations.len(), 1);
}

#[test]
fn active_giftwrap_interest_reuses_one_id_across_accounts() {
    let alice = active_giftwrap_inbox_interest("alice");
    let bob = active_giftwrap_inbox_interest("bob");
    assert_eq!(alice.id, bob.id, "account switch replaces one slot");
    assert_eq!(alice.id, active_giftwrap_inbox_interest_id());
    assert!(matches!(alice.scope, InterestScope::ActiveAccount));
    assert_eq!(alice.shape.p_tag_routing, PTagRouting::Nip17DmRelays);
    assert_eq!(bob.shape.p_tag_routing, PTagRouting::Nip17DmRelays);
    assert!(alice
        .shape
        .tags
        .get("p")
        .map(|s| s.contains("alice"))
        .unwrap_or(false));
    assert!(bob
        .shape
        .tags
        .get("p")
        .map(|s| s.contains("bob"))
        .unwrap_or(false));
}

#[test]
fn snapshot_round_trips_through_serde() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);
    inbox.ingest_gift_wrap(
        &gift_wrapped_dm(&alice, &bob.public_key(), "hi", 100, None).as_json(),
        None,
    );
    let snap = inbox.snapshot();
    let encoded = serde_json::to_string(&snap).expect("serialises");
    let decoded: DmInboxSnapshot = serde_json::from_str(&encoded).expect("deserialises");
    assert_eq!(snap, decoded);
}

