//! LMDB-backend addr-tombstone GC tests (S-2 audit fix).
//!
//! Verifies that `gc_step` purges stale `addr_tombstone` entries and retains
//! fresh ones. Split from `tests.rs` to stay under the 500-LOC hard cap.

#![cfg(feature = "lmdb-backend")]

use nostr::prelude::*;

use crate::types::RawEvent;
use crate::EventStore;

use super::test_fixtures::{open_tmp, signed_event_with_keys, verified};

/// Insert a kind:5 with an `a`-tag to create an addr tombstone, then run
/// gc_step far into the future — the stale addr tombstone must be purged.
///
/// This is the RED → GREEN proof for the S-2 audit finding.
#[test]
fn lmdb_stale_addr_tombstone_is_purged_by_gc() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let pk_hex = keys.public_key().to_hex();

    // A parameterized-replaceable event (kind 30023) that gets deleted.
    let target = signed_event_with_keys(&keys, 30023, 1000, "article", Some("my-slug"));
    store
        .insert(verified(target), &"wss://r/".into(), 1_000_000)
        .unwrap();

    // kind:5 with an `a`-tag deleting the coordinate.
    let a_tag_value = format!("30023:{pk_hex}:my-slug");
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::parse(["a", &a_tag_value]).unwrap())
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_json = k5.try_as_json().unwrap();
    let k5_raw: RawEvent = serde_json::from_str(&k5_json).unwrap();
    store
        .insert(verified(k5_raw), &"wss://r/".into(), 2_000_000)
        .unwrap();

    // Addr tombstone must exist after the kind:5 insert.
    let count_before = store.addr_tombstone_count().unwrap();
    assert!(
        count_before >= 1,
        "addr tombstone must be written by kind:5 a-tag insert"
    );

    // GC with now_secs = deleted_at + TOMBSTONE_MAX_AGE_SECS + 1.
    // deleted_at = 2000 (kind5.created_at); age window = 90 * 24 * 3600.
    const MAX_AGE: u64 = 90 * 24 * 3600;
    let now_secs = 2000 + MAX_AGE + 1;
    let budget = crate::types::GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 10_000,
        max_total_events: usize::MAX,
    };
    let report = store.gc_step(budget, now_secs).unwrap();

    assert_eq!(
        store.addr_tombstone_count().unwrap(),
        0,
        "stale addr_tombstone must be purged by gc_step"
    );
    assert_eq!(
        report.addr_tombstones_purged, 1,
        "report must count purged addr_tombstone"
    );
}

/// A fresh addr tombstone (well within TOMBSTONE_MAX_AGE_SECS) must NOT be purged.
#[test]
fn lmdb_fresh_addr_tombstone_is_retained_by_gc() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let pk_hex = keys.public_key().to_hex();

    let target = signed_event_with_keys(&keys, 30023, 1000, "article", Some("keep-slug"));
    store
        .insert(verified(target), &"wss://r/".into(), 1_000_000)
        .unwrap();

    let a_tag_value = format!("30023:{pk_hex}:keep-slug");
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::parse(["a", &a_tag_value]).unwrap())
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_json = k5.try_as_json().unwrap();
    let k5_raw: RawEvent = serde_json::from_str(&k5_json).unwrap();
    store
        .insert(verified(k5_raw), &"wss://r/".into(), 2_000_000)
        .unwrap();

    // GC with now_secs = deleted_at + 1 (far below the 90-day threshold).
    let budget = crate::types::GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 10_000,
        max_total_events: usize::MAX,
    };
    let report = store.gc_step(budget, 2001).unwrap();

    assert!(
        store.addr_tombstone_count().unwrap() >= 1,
        "fresh addr_tombstone must NOT be purged"
    );
    assert_eq!(
        report.addr_tombstones_purged, 0,
        "report must not count fresh addr_tombstone as purged"
    );
}
