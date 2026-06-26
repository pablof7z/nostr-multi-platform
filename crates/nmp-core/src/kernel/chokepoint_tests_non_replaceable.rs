//! ADR-0057 §7 oracles: read-your-writes for non-replaceable kinds.

use super::*;

fn signed_kind(keys: &::nostr::Keys, kind: u32, content: &str, created_at: u64) -> SignedEvent {
    let event = ::nostr::EventBuilder::new(::nostr::Kind::from(kind as u16), content)
        .custom_created_at(::nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("generated keys sign");
    SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: event.kind.as_u16() as u32,
            tags: event
                .tags
                .iter()
                .map(|tag: &::nostr::Tag| tag.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

/// ADR-0057 oracle (#1440) — a locally-published kind:1 NOTE is delivered to
/// app observers and persisted immediately BEFORE any relay ACK. The deleted
/// `record_local_publish_intent` ladder explicitly skipped non-replaceables, so
/// a just-posted note could disappear until relay echo. Routing the local
/// publish through the chokepoint persists it AND fires app-observer delivery
/// on the spot; feed rendering is owned by registered feed interests.
#[test]
fn local_kind1_note_read_your_writes_before_relay_ack() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_kind(&keys, 1, "my first post", 1_700_000_000);
    let event_id = signed.id.clone();

    let slot = new_event_observer_slot();
    let observer = CapturingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);
    kernel.active_account = Some(author.clone());
    // Keep the author in `timeline_authors`; local publish no longer writes the
    // home timeline cache directly, so the assertions below are about the
    // core-owned chokepoint effects.
    kernel.timeline_authors.insert(author.clone());
    kernel.seed_kind10002_for_test(&author, &["wss://write.test"]);

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert!(!outbound.is_empty(), "publish should have an outbox target");

    // Read-your-writes: the note fired the observer exactly once and is
    // persisted — BEFORE any relay OK.
    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "a locally published kind:1 note must fire the observer exactly once (read-your-writes)"
    );
    let id_bytes = crate::kernel::hex_to_pubkey_bytes(&event_id).expect("event id is 64-char hex");
    assert!(
        kernel
            .store
            .get_by_id(&id_bytes)
            .expect("store get_by_id must not error")
            .is_some(),
        "the locally published note must be persisted immediately"
    );
}

/// ADR-0057 oracle (#1440) — a locally-published kind:7 REACTION is delivered to
/// app observers immediately and PERSISTED, even though it is neither a
/// follow-feed timeline kind nor a replaceable (the old ladder dropped it).
#[test]
fn local_kind7_reaction_read_your_writes() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_kind(&keys, 7, "+", 1_700_000_000);
    let event_id = signed.id.clone();

    let slot = new_event_observer_slot();
    let observer = CapturingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);
    kernel.active_account = Some(author.clone());
    kernel.seed_kind10002_for_test(&author, &["wss://write.test"]);

    let _ = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "a locally published kind:7 reaction must fire the observer exactly once"
    );
    let id_bytes = crate::kernel::hex_to_pubkey_bytes(&event_id).expect("event id is 64-char hex");
    assert!(
        kernel
            .store
            .get_by_id(&id_bytes)
            .expect("store get_by_id must not error")
            .is_some(),
        "a locally published kind:7 reaction must be persisted (admission = valid-sig)"
    );
}

/// ADR-0057 oracle — the read-your-writes pin source. A locally-published note
/// from an author NOT in their own follow set (so it is NOT pinned by the
/// `self.timeline` clause) must still be pinned by the publish-in-flight pin
/// source in `derive_store_pin_set` so LRU cannot evict it before the relay echo
/// dedups against it.
#[test]
fn locally_published_event_is_pinned_until_relay_confirmation() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    // kind:1 note, but NO follow-feed kinds declared and author NOT in
    // timeline_authors → the note is persisted but NOT in `self.timeline`, so
    // the timeline pin clause does not cover it. Only the publish-in-flight pin
    // source can keep it alive.
    let signed = signed_kind(&keys, 1, "unpinned-by-timeline note", 1_700_000_000);
    let event_id = signed.id.clone();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(author.clone());
    kernel.seed_kind10002_for_test(&author, &["wss://write.test"]);

    let _ = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);

    // The note is not in the timeline read-cache (no follow-feed kind declared).
    assert!(
        !kernel.timeline.iter().any(|id| id == &event_id),
        "precondition: the note must NOT be in the timeline (so timeline pin clause cannot cover it)"
    );

    // …yet the publish-in-flight pin source pins it.
    let id_bytes = crate::kernel::hex_to_pubkey_bytes(&event_id).expect("event id is 64-char hex");
    let mut id32 = [0u8; 32];
    id32.copy_from_slice(&id_bytes);
    let (pins, _complete) = kernel.derive_store_pin_set();
    assert!(
        pins.contains(&id32),
        "ADR-0057: an in-flight locally-published event must be pinned until relay confirmation \
         / terminal settlement so it is not LRU-evicted before its relay echo"
    );
}
