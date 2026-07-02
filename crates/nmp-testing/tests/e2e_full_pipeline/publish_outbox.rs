//! Test 3 — publish_roundtrip_via_outbox
//!
//! Scenario:
//!   1. Build a PublishEngine with a StaticOutbox carrying alice's write relays.
//!   2. Dispatch a kind:1 publish via the engine.
//!   3. Assert the ReplayDispatcher received EVENT frames for alice's relays.
//!   4. Assert the signed event carries kind=1.
//!
//! The PublishEngine + ReplayDispatcher IS the full write-path observable at
//! the framework layer (M6/M7/M8).  Router identity canonicalization (trailing
//! slash) is an active contract as per publish_relay_identity_tests.rs.

use crate::support::padded_pubkey;

#[test]
fn publish_roundtrip_via_outbox() {
    use nmp_core::publish::{
        InMemoryPublishStore, PublishAction, PublishEngine, PublishTarget, RelayAck, RelayUrl,
        ReplayDispatcher, RetryPolicy, StaticOutbox,
    };
    use nmp_signer_iface::{SignedEvent, UnsignedEvent};
    use std::sync::Arc;

    // Alice's NIP-65 outbox write relays (wire form with trailing slash).
    let alice_writes: Vec<RelayUrl> = vec!["wss://r1/".to_string(), "wss://r2/".to_string()];
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert(padded_pubkey("alice"), alice_writes.clone());

    let dispatcher = Arc::new(ReplayDispatcher::new());
    // Script OK acks under the canonical relay keys (engine canonicalizes trailing slash).
    dispatcher.script("wss://r1", vec![RelayAck::ok("wss://r1")]);
    dispatcher.script("wss://r2", vec![RelayAck::ok("wss://r2")]);

    let mut engine = PublishEngine::new(
        Arc::new(outbox),
        Arc::clone(&dispatcher) as Arc<dyn nmp_core::publish::RelayDispatcher>,
        Arc::new(InMemoryPublishStore::new()),
        RetryPolicy::default(),
    );

    // A minimal kind:1 signed event authored by alice.
    let event = SignedEvent {
        id: "b".repeat(64),
        sig: "c".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: padded_pubkey("alice"),
            kind: 1,
            tags: vec![],
            content: "hello".to_string(),
            created_at: 1_700_000_100,
        },
    };

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "test-h1".to_string(),
                event,
                target: PublishTarget::Auto,
            },
            0,
            None,
        )
        .expect("public publish must succeed");

    // The dispatcher must have received frames on both outbox relays.
    let sent = dispatcher.sent_frames();
    let sent_relays: std::collections::BTreeSet<&str> =
        sent.iter().map(|(url, _)| url.as_str()).collect();
    assert!(
        sent_relays.contains("wss://r1"),
        "kind:1 event must be dispatched to alice's canonical write relay r1; got: {sent_relays:?}"
    );
    assert!(
        sent_relays.contains("wss://r2"),
        "kind:1 event must be dispatched to alice's canonical write relay r2; got: {sent_relays:?}"
    );

    // Confirm the dispatched frames encode a kind:1 event.
    // Sent frames are `["EVENT", <signed-event-json>]` strings.
    let all_text: String = sent
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        all_text.contains("\"kind\":1"),
        "dispatched frame must encode kind:1; got excerpt: {}",
        &all_text[..std::cmp::min(200, all_text.len())]
    );
}
