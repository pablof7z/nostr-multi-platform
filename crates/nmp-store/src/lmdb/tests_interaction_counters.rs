//! Integration tests for the interaction-counter sidecar (issue #1519).
//!
//! Tests cover: basic increment, multi-event increment, decrement on kind:5
//! deletion, decrement on GC, decrement on delete_by_filter, zero-row cleanup.

#![cfg(all(test, feature = "lmdb-backend"))]

use tempfile::tempdir;

use crate::types::{GcBudget, RawEvent, VerifiedEvent};
use crate::{EventStore, LmdbEventStore, TargetInteractionCounts};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn open_tmp() -> (LmdbEventStore, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let store = LmdbEventStore::open(dir.path()).expect("open");
    (store, dir)
}

/// A canonical 64-hex-char target event id (all zeros except last byte=1)
/// for use when we don't actually insert the target.
const PHANTOM_TARGET: &str = "0000000000000000000000000000000000000000000000000000000000000001";

fn phantom_target_id() -> crate::types::EventId {
    crate::types::hex_to_event_id(PHANTOM_TARGET).expect("valid hex")
}

fn verified(raw: RawEvent) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(raw)
}

/// Build a signed reply (kind:1) with an e-tag pointing at `target_hex`.
fn signed_reply(target_hex: &str, created_at: u64) -> RawEvent {
    use nostr::prelude::*;
    let keys = Keys::generate();
    let target_id = nostr::EventId::from_hex(target_hex).expect("valid hex");
    let ev = EventBuilder::new(Kind::from(1u16), "reply")
        .custom_created_at(Timestamp::from_secs(created_at))
        .tag(Tag::event(target_id))
        .sign_with_keys(&keys)
        .expect("sign");
    let json = ev.try_as_json().expect("json");
    serde_json::from_str(&json).expect("parse")
}

/// Build a signed reaction (kind:7) with an e-tag pointing at `target_hex`.
fn signed_reaction(target_hex: &str, created_at: u64) -> RawEvent {
    use nostr::prelude::*;
    let keys = Keys::generate();
    let target_id = nostr::EventId::from_hex(target_hex).expect("valid hex");
    let ev = EventBuilder::new(Kind::from(7u16), "+")
        .custom_created_at(Timestamp::from_secs(created_at))
        .tag(Tag::event(target_id))
        .sign_with_keys(&keys)
        .expect("sign");
    let json = ev.try_as_json().expect("json");
    serde_json::from_str(&json).expect("parse")
}

/// Build a signed repost (kind:6) with an e-tag pointing at `target_hex`.
fn signed_repost(target_hex: &str, created_at: u64) -> RawEvent {
    use nostr::prelude::*;
    let keys = Keys::generate();
    let target_id = nostr::EventId::from_hex(target_hex).expect("valid hex");
    let ev = EventBuilder::new(Kind::from(6u16), "")
        .custom_created_at(Timestamp::from_secs(created_at))
        .tag(Tag::event(target_id))
        .sign_with_keys(&keys)
        .expect("sign");
    let json = ev.try_as_json().expect("json");
    serde_json::from_str(&json).expect("parse")
}

/// Build a signed plain note (kind:1, no e-tag).
fn signed_note(created_at: u64) -> RawEvent {
    use nostr::prelude::*;
    let keys = Keys::generate();
    let ev = EventBuilder::new(Kind::from(1u16), "hello")
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(&keys)
        .expect("sign");
    let json = ev.try_as_json().expect("json");
    serde_json::from_str(&json).expect("parse")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// TC-1: Inserting a reply increments the reply counter.
#[test]
fn tc1_reply_increments_counter() {
    let (store, _dir) = open_tmp();
    let reply = signed_reply(PHANTOM_TARGET, 1000);
    store.insert(verified(reply), &"wss://r/".into(), 1000_000).unwrap();
    let counts = store.interaction_counts(&phantom_target_id()).unwrap();
    assert_eq!(counts.replies, 1, "one reply must increment replies to 1");
    assert_eq!(counts.reactions, 0);
    assert_eq!(counts.reposts, 0);
    assert_eq!(counts.zaps, 0);
}

/// TC-2: Inserting a reaction increments the reaction counter.
#[test]
fn tc2_reaction_increments_counter() {
    let (store, _dir) = open_tmp();
    let reaction = signed_reaction(PHANTOM_TARGET, 1000);
    store.insert(verified(reaction), &"wss://r/".into(), 1000_000).unwrap();
    let counts = store.interaction_counts(&phantom_target_id()).unwrap();
    assert_eq!(counts.reactions, 1);
    assert_eq!(counts.replies, 0);
}

/// TC-3: Inserting a repost increments the repost counter.
#[test]
fn tc3_repost_increments_counter() {
    let (store, _dir) = open_tmp();
    let repost = signed_repost(PHANTOM_TARGET, 1000);
    store.insert(verified(repost), &"wss://r/".into(), 1000_000).unwrap();
    let counts = store.interaction_counts(&phantom_target_id()).unwrap();
    assert_eq!(counts.reposts, 1);
}

/// TC-4: Multiple interactions accumulate independently.
#[test]
fn tc4_multiple_interactions_accumulate() {
    let (store, _dir) = open_tmp();
    let relay = "wss://r/".to_string();

    for i in 0..3u64 {
        store.insert(verified(signed_reply(PHANTOM_TARGET, 1000 + i)), &relay, 1000_000).unwrap();
    }
    for i in 0..2u64 {
        store.insert(verified(signed_reaction(PHANTOM_TARGET, 2000 + i)), &relay, 2000_000).unwrap();
    }
    store.insert(verified(signed_repost(PHANTOM_TARGET, 3000)), &relay, 3000_000).unwrap();

    let counts = store.interaction_counts(&phantom_target_id()).unwrap();
    assert_eq!(counts.replies, 3);
    assert_eq!(counts.reactions, 2);
    assert_eq!(counts.reposts, 1);
    assert_eq!(counts.zaps, 0);
}

/// TC-5: Non-interaction kinds do NOT affect counters.
#[test]
fn tc5_non_interaction_kinds_ignored() {
    let (store, _dir) = open_tmp();
    // Insert a plain note (kind:1 with no e-tag).
    store.insert(verified(signed_note(1000)), &"wss://r/".into(), 1000_000).unwrap();
    let counts = store.interaction_counts(&phantom_target_id()).unwrap();
    assert_eq!(counts, TargetInteractionCounts::default());
}

/// TC-6: kind:5 deletion of a reply decrements the reply counter.
#[test]
fn tc6_kind5_delete_decrements() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let relay = "wss://r/".to_string();

    // Insert a reply from Alice.
    let alice = Keys::generate();
    let target_event_id = nostr::EventId::from_hex(PHANTOM_TARGET).expect("valid hex");
    let reply_ev = EventBuilder::new(Kind::from(1u16), "hello")
        .custom_created_at(Timestamp::from_secs(1000))
        .tag(Tag::event(target_event_id))
        .sign_with_keys(&alice)
        .expect("sign");
    let reply_json = reply_ev.try_as_json().expect("json");
    let reply_raw: RawEvent = serde_json::from_str(&reply_json).expect("parse");
    let reply_id_hex = reply_raw.id.clone();
    store.insert(verified(reply_raw), &relay, 1000_000).unwrap();

    // Verify counter is 1.
    assert_eq!(store.interaction_counts(&phantom_target_id()).unwrap().replies, 1);

    // Alice sends a kind:5 deleting her reply.
    let reply_nostr_id = nostr::EventId::from_hex(&reply_id_hex).expect("valid hex");
    let del_ev = EventBuilder::new(Kind::from(5u16), "")
        .custom_created_at(Timestamp::from_secs(2000))
        .tag(Tag::event(reply_nostr_id))
        .sign_with_keys(&alice)
        .expect("sign");
    let del_json = del_ev.try_as_json().expect("json");
    let del_raw: RawEvent = serde_json::from_str(&del_json).expect("parse");
    store.insert(verified(del_raw), &relay, 2000_000).unwrap();

    // Counter must be back to 0.
    assert_eq!(store.interaction_counts(&phantom_target_id()).unwrap().replies, 0);
}

/// TC-7: GC Phase 1 (NIP-40 expiry) decrements counter when evicting an
/// interaction event.
#[test]
fn tc7_gc_expiry_decrements() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let relay = "wss://r/".to_string();

    // Insert a reaction with an expiration tag in the past (from GC's perspective).
    let keys = Keys::generate();
    let target_event_id = nostr::EventId::from_hex(PHANTOM_TARGET).expect("valid hex");
    let ev = EventBuilder::new(Kind::from(7u16), "+")
        .custom_created_at(Timestamp::from_secs(1000))
        .tag(Tag::event(target_event_id))
        .tag(Tag::expiration(Timestamp::from_secs(5000)))
        .sign_with_keys(&keys)
        .expect("sign");
    let json = ev.try_as_json().expect("json");
    let raw: RawEvent = serde_json::from_str(&json).expect("parse");
    store.insert(verified(raw), &relay, 1000_000).unwrap();

    assert_eq!(store.interaction_counts(&phantom_target_id()).unwrap().reactions, 1);

    let budget = GcBudget {
        max_events_per_step: 100,
        max_duration_ms: 60_000,
        max_total_events: usize::MAX,
    };
    store.gc_step(budget, 6000).unwrap(); // now(6000) > expiry(5000)

    assert_eq!(
        store.interaction_counts(&phantom_target_id()).unwrap().reactions,
        0,
        "GC expiry must decrement counter"
    );
}

/// TC-8: delete_by_filter decrements counter for deleted interaction events.
#[test]
fn tc8_delete_by_filter_decrements() {
    use crate::types::DeleteFilter;
    let (store, _dir) = open_tmp();
    let relay = "wss://r/".to_string();

    let reply = signed_reply(PHANTOM_TARGET, 1000);
    let reply_id = reply.id_bytes().unwrap();
    store.insert(verified(reply), &relay, 1000_000).unwrap();

    assert_eq!(store.interaction_counts(&phantom_target_id()).unwrap().replies, 1);

    store.delete_by_filter(DeleteFilter::ByIds(vec![reply_id])).unwrap();

    assert_eq!(
        store.interaction_counts(&phantom_target_id()).unwrap().replies,
        0,
        "delete_by_filter must decrement counter"
    );
}
