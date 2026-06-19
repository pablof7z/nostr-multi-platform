//! Test 10 from V-118 GC regression suite (500-LOC cap split from
//! `tests_gc.rs`): bulk `DeleteFilter::ByAuthor` must remove expiry-index
//! entries so that no orphaned entries cause phantom reaps on the next gc pass.

#![cfg(feature = "lmdb-backend")]

use crate::types::GcBudget;
use crate::EventStore;

use super::test_fixtures::{open_tmp, verified};

/// Bulk `DeleteFilter::ByAuthor` must remove expiry-index entries for each
/// deleted event so that no orphaned entries cause phantom reaps on the next
/// gc pass.
///
/// Proof: insert 4 events from one author (2 with expiry tags, 2 without) plus
/// 1 expiring event from a DIFFERENT author.  Delete all events from the first
/// author.  Run gc_step past the expiry timestamps.  Assert only 1 event reaped
/// (the other author's — no orphaned index entries from the deleted author).
#[test]
fn v118_bulk_delete_by_author_removes_index_entries() {
    use nostr::prelude::*;

    let (store, _dir) = open_tmp();

    let base_ts = 1_700_000_000u64;
    let exp_ts_x = base_ts + 50;
    let exp_ts_y = base_ts + 100;
    let gc_now = base_ts + 200; // past both expiry timestamps

    let keys_victim = Keys::generate();
    let keys_other = Keys::generate();

    // Insert 2 expiring events from the victim author.
    let mut victim_pubkey_bytes = [0u8; 32];
    for i in 0..2usize {
        let exp_ts = if i == 0 { exp_ts_x } else { exp_ts_y };
        let ev = EventBuilder::text_note(format!("expiring-victim-{i}"))
            .custom_created_at(Timestamp::from_secs(base_ts + i as u64))
            .tag(Tag::expiration(Timestamp::from_secs(exp_ts)))
            .sign_with_keys(&keys_victim)
            .expect("sign");
        let json = ev.try_as_json().expect("json");
        let raw: crate::types::RawEvent = serde_json::from_str(&json).expect("parse");
        victim_pubkey_bytes.copy_from_slice(&raw.pubkey_bytes().expect("pk"));
        store
            .insert(verified(raw), &"wss://r/".into(), base_ts * 1_000)
            .expect("insert victim expiring");
    }
    // Insert 2 non-expiring events from the same victim author.
    for i in 0..2usize {
        let ev = EventBuilder::text_note(format!("plain-victim-{i}"))
            .custom_created_at(Timestamp::from_secs(base_ts + 10 + i as u64))
            .sign_with_keys(&keys_victim)
            .expect("sign");
        let json = ev.try_as_json().expect("json");
        let raw: crate::types::RawEvent = serde_json::from_str(&json).expect("parse");
        store
            .insert(verified(raw), &"wss://r/".into(), base_ts * 1_000)
            .expect("insert victim plain");
    }

    // Insert 1 expiring event from a DIFFERENT author (must survive bulk delete).
    let ev_other = EventBuilder::text_note("expiring-other")
        .custom_created_at(Timestamp::from_secs(base_ts))
        .tag(Tag::expiration(Timestamp::from_secs(exp_ts_x)))
        .sign_with_keys(&keys_other)
        .expect("sign");
    let json_other = ev_other.try_as_json().expect("json");
    let raw_other: crate::types::RawEvent = serde_json::from_str(&json_other).expect("parse");
    let id_other = raw_other.id_bytes().expect("id");
    store
        .insert(verified(raw_other), &"wss://r/".into(), base_ts * 1_000)
        .expect("insert other");

    // Bulk-delete all events from the victim author.
    let deleted = store
        .delete_by_filter(crate::types::DeleteFilter::ByAuthor(victim_pubkey_bytes))
        .expect("delete_by_filter");
    assert!(deleted >= 4, "expected at least 4 deletions, got {deleted}");

    // gc_step past both expiry timestamps.
    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 60_000,
        max_total_events: usize::MAX,
    };
    let report = store.gc_step(budget, gc_now).expect("gc_step");

    // The victim's expiring events are already deleted — their expiry-index
    // entries must have been removed by delete_by_filter, so gc_step must NOT
    // reap them as phantoms.  Only the other author's event is reaped (1 event).
    assert_eq!(
        report.expired_reaped, 1,
        "bulk delete must have removed victim's expiry-index entries; \
         only the other author's event should be reaped. expired_reaped={}",
        report.expired_reaped,
    );

    // The other author's event must now be gone (reaped by gc).
    assert!(
        store.get_by_id(&id_other).expect("get_by_id").is_none(),
        "other author's event must be reaped by gc",
    );
}
