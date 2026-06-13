//! #1090 Stage 2 — floor-coherent store-tier eviction pins.
//!
//! ## The hole this closes
//!
//! The live `since`-floor for a subscription is content-derived: the
//! `watermark_fn` (`kernel/mod.rs`) queries the store for the newest stored
//! event matching each REQ shape and floors that REQ's `since` to `floor + 1`,
//! so the relay does not re-emit events already on disk.
//!
//! LRU eviction (Stage 3 ceiling) is free to delete a *middle* event that is
//! older than the surviving newest event. The floor stays at `newest + 1`, so
//! the self-healing REQ — which only asks for events newer than the floor —
//! will NEVER re-request that middle event: a permanent hole.
//!
//! The fix: `Kernel::derive_store_pin_set` additionally pins every stored event
//! at or below each active floored shape's `since`-floor, so the middle event
//! is never an eviction candidate while the shape's floor sits above it.
//!
//! These tests register a real active `LogicalInterest` (via the same
//! generic `open_interest_sub` seam the actor uses) and ingest events through
//! the real pre-verified ingest path, then assert the derived store pin set.

use super::super::ram_eviction_tests::{make_pubkey, pin_clock, T0_SECS};
use super::super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::{RawEvent, VerifiedEvent};

/// Register a generic `open_interest` on the kernel from a verbatim NIP-01
/// filter — mirrors the `ActorCommand::OpenInterest` dispatch arm body. Copied
/// from `ram_eviction_view_pin_tests` because the dispatch helper is private to
/// the actor module and these tests exercise the kernel pin invariant directly.
fn open_interest(kernel: &mut Kernel, filter_json: &str, consumer_id: &str) {
    use crate::planner::{InterestLifecycle, InterestScope, LogicalInterest};
    use crate::subs::sub_key::{SubIdentity, SubKey, SubOwnerKey, SubScope};

    let shape = crate::planner::InterestShape::from_filter_json(filter_json)
        .expect("test filter must be a valid NIP-01 filter object");
    let key = SubKey::builder("open-interest")
        .with(&shape)
        .with(1u32)
        .finish();
    let identity = SubIdentity::new(SubOwnerKey::new(consumer_id), key, SubScope::Global);
    let interest = LogicalInterest {
        scope: InterestScope::Global,
        shape,
        lifecycle: InterestLifecycle::Tailing,
        ..LogicalInterest::default()
    };
    let _ = kernel.open_interest_sub(identity, interest);
}

/// Ingest one kind:1 note through the real pre-verified ingest path so it lands
/// in BOTH the RAM `events` map and the authoritative `self.store`.
fn inject_note(kernel: &mut Kernel, id: &str, pubkey: &str, created_at: u64) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: format!("note {id}"),
        sig: "a".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    kernel.ingest_pre_verified_event(RelayRole::Content, "", verified);
}

/// Convert a 64-hex id string into a store `EventId` ([u8; 32]).
fn id_bytes(hex: &str) -> crate::store::EventId {
    let parsed = ::nostr::prelude::EventId::from_hex(hex).expect("valid hex id");
    let mut out = [0u8; 32];
    out.copy_from_slice(parsed.as_bytes());
    out
}

/// The core Stage-2 invariant: for an active author+kind interest whose floor
/// sits at the newest stored event, EVERY older stored event matching the shape
/// (the "middle" and "old" events below the floor) is in the derived store pin
/// set — so LRU eviction can never punch a hole the floored REQ won't re-fetch.
#[test]
fn derive_store_pin_set_pins_events_below_shape_floor() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(7_001);
    let e_old = format!("{:0>64x}", 0xF00001u64);
    let e_mid = format!("{:0>64x}", 0xF00002u64);
    let e_new = format!("{:0>64x}", 0xF00003u64);

    inject_note(&mut kernel, &e_old, &author, 100);
    inject_note(&mut kernel, &e_mid, &author, 200);
    inject_note(&mut kernel, &e_new, &author, 300);

    // Register an active floored author+kind interest (kind:1). The store's
    // newest matching event (created_at=300) sets this shape's `since`-floor.
    open_interest(
        &mut kernel,
        &format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#),
        "floor-coherent-test",
    );

    // Simulate prior RAM-tier eviction: drop the events from the RAM `events`
    // map so the STORE is their sole holder. This is the exact hole scenario —
    // `open_view_pins` scans only RAM, so without the Stage-2 store-scan the
    // below-floor events would NOT be pinned and store LRU could evict them
    // permanently (the floored REQ asks only for created_at > 300).
    kernel.events.clear();

    let pins = kernel.derive_store_pin_set();

    // The newest event would survive LRU on its own merit; the hole risk is
    // the OLD and MID events, both below the floor (300). Stage 2 must pin them
    // from the store scan even though they are absent from the RAM map.
    assert!(
        pins.contains(&id_bytes(&e_old)),
        "e_old (created_at=100, below floor=300) must be pinned from the store scan"
    );
    assert!(
        pins.contains(&id_bytes(&e_mid)),
        "e_mid (created_at=200, below floor=300) must be pinned from the store scan"
    );
}

/// A stored event for an author with NO active interest must NOT be pinned by
/// the floor-coherent extension (it has no floored shape to protect it).
#[test]
fn derive_store_pin_set_does_not_pin_events_with_no_active_interest() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    // Author A: floored interest active.
    let author_a = make_pubkey(7_101);
    let a_old = format!("{:0>64x}", 0xE00001u64);
    let a_new = format!("{:0>64x}", 0xE00002u64);
    inject_note(&mut kernel, &a_old, &author_a, 100);
    inject_note(&mut kernel, &a_new, &author_a, 300);
    open_interest(
        &mut kernel,
        &format!(r#"{{"kinds":[1],"authors":["{author_a}"]}}"#),
        "floor-coherent-test",
    );

    // Author B: NO active interest. Its cold event must not be pinned.
    let author_b = make_pubkey(7_202);
    let b_cold = format!("{:0>64x}", 0xE00099u64);
    inject_note(&mut kernel, &b_cold, &author_b, 150);

    // Drop RAM holders so only the store + the floor-coherent scan can pin.
    kernel.events.clear();

    let pins = kernel.derive_store_pin_set();

    assert!(
        pins.contains(&id_bytes(&a_old)),
        "author A's below-floor event must be pinned"
    );
    assert!(
        !pins.contains(&id_bytes(&b_cold)),
        "author B has no active interest — its event must NOT be pinned"
    );
}
