//! Exit-gate test 2 — Message round-trip.
//!
//! Covers: send a message; peer receives and decrypts identical plaintext.
//!
//! Exit gate spec (marmot-mls.md §"Exit gate (product)"):
//!   "send a message, peer receives and decrypts it."

#[path = "marmot_harness.rs"]
mod harness;

use mdk_core::prelude::MessageProcessingResult;
use nostr::{EventBuilder, Keys, Kind};

#[test]
fn message_roundtrip_alice_sends_bob_decrypts() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();

    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());

    // Establish group (both services at same epoch after post-join self_update).
    let group_id = harness::setup_two_member_group(
        &alice, &alice_keys,
        &bob, &bob_keys,
        "msg-roundtrip",
    );

    // ── Alice sends a message ─────────────────────────────────────────────────
    let plaintext = "hello bob, this is alice";
    let rumor = EventBuilder::new(Kind::TextNote, plaintext)
        .build(alice_keys.public_key());
    let msg_event = alice
        .create_message(&group_id, rumor)
        .expect("alice create_message");

    // Contract: the encrypted event is kind:445 (MLS group message).
    assert_eq!(msg_event.kind, Kind::MlsGroupMessage);

    // ── Bob decrypts it ───────────────────────────────────────────────────────
    match bob
        .process_message(&msg_event)
        .expect("bob process_message")
    {
        MessageProcessingResult::ApplicationMessage(m) => {
            assert_eq!(m.content, plaintext, "plaintext must round-trip identically");
            assert_eq!(
                m.pubkey,
                alice_keys.public_key(),
                "sender pubkey must match Alice"
            );
        }
        other => panic!("expected ApplicationMessage, got {other:?}"),
    }
}

/// Multiple messages in sequence all decrypt correctly.
#[test]
fn message_roundtrip_multiple_messages() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();

    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());

    let group_id = harness::setup_two_member_group(
        &alice, &alice_keys,
        &bob, &bob_keys,
        "msg-roundtrip-multi",
    );

    let messages = ["first", "second", "third", "fourth", "fifth"];

    for msg in &messages {
        let rumor = EventBuilder::new(Kind::TextNote, *msg)
            .build(alice_keys.public_key());
        let event = alice
            .create_message(&group_id, rumor)
            .expect("alice create_message");

        match bob.process_message(&event).expect("bob process_message") {
            MessageProcessingResult::ApplicationMessage(m) => {
                assert_eq!(m.content, *msg, "message {msg} must round-trip");
            }
            other => panic!("expected ApplicationMessage for {msg:?}, got {other:?}"),
        }
    }
}

/// Bob can also send; Alice decrypts.
#[test]
fn message_roundtrip_bob_sends_alice_decrypts() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();

    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());

    let group_id = harness::setup_two_member_group(
        &alice, &alice_keys,
        &bob, &bob_keys,
        "msg-roundtrip-bob-sends",
    );

    let plaintext = "hello alice, this is bob";
    let rumor = EventBuilder::new(Kind::TextNote, plaintext)
        .build(bob_keys.public_key());
    let msg_event = bob
        .create_message(&group_id, rumor)
        .expect("bob create_message");

    match alice.process_message(&msg_event).expect("alice process_message") {
        MessageProcessingResult::ApplicationMessage(m) => {
            assert_eq!(m.content, plaintext);
            assert_eq!(m.pubkey, bob_keys.public_key());
        }
        other => panic!("expected ApplicationMessage, got {other:?}"),
    }
}

/// get_messages returns history consistent with what was exchanged.
#[test]
fn message_roundtrip_get_messages_history() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();

    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());

    let group_id = harness::setup_two_member_group(
        &alice, &alice_keys,
        &bob, &bob_keys,
        "msg-history",
    );

    let texts = ["msg-a", "msg-b", "msg-c"];
    for text in &texts {
        let rumor = EventBuilder::new(Kind::TextNote, *text)
            .build(alice_keys.public_key());
        let event = alice.create_message(&group_id, rumor).expect("create_message");
        // Bob must process each event so MDK stores it in history.
        bob.process_message(&event).expect("bob process_message");
    }

    let history = bob.get_messages(&group_id).expect("bob get_messages");
    assert_eq!(history.len(), texts.len(), "history must contain all messages");

    // MDK does not guarantee insertion-order retrieval; verify all messages are
    // present regardless of order (set containment check).
    let history_contents: std::collections::HashSet<&str> =
        history.iter().map(|m| m.content.as_str()).collect();
    for expected in &texts {
        assert!(
            history_contents.contains(expected),
            "history must contain message {:?}",
            expected
        );
    }
}
