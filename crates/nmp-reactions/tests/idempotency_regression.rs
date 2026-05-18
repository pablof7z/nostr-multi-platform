//! Idempotency regression — the nip23-stale-redelivery analogue for regular
//! (non-replaceable) events.
//!
//! Kinds 7/6/16 are NOT replaceable, so there is no `(author, d_tag)`
//! supersession. The correctness property instead is: ingesting the SAME
//! reaction `event_id` twice must not double-count. The primary key is the
//! reaction's own immutable id, so the second route rewrites the identical
//! row and `reaction_summary` stays at 1.

mod common;

use common::reaction;
use nmp_core::store::{EventStore, MemEventStore};
use nmp_reactions::{decode_and_route, reaction_summary, ReactionTarget, NAMESPACE};

const TARGET: &str = "target-event-id-000000000000000000000000000000000000000000000000000";
const TARGET_AUTHOR: &str = "target-author-00000000000000000000000000000000000000000000000000";
const ALICE: &str = "alice-000000000000000000000000000000000000000000000000000000000000";

#[test]
fn same_event_id_ingested_twice_counts_once() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let evt = reaction(&"a".repeat(64), ALICE, 100, TARGET, TARGET_AUTHOR, "👍");

    decode_and_route(&evt, &handle).unwrap();
    // Redeliver the identical event (reconnect backfill / multi-relay fan-in).
    decode_and_route(&evt, &handle).unwrap();

    let target = ReactionTarget::Event(TARGET.to_string());
    let summary = reaction_summary(&handle, &target).unwrap();
    assert_eq!(summary.total, 1, "duplicate id must not double-count");
    assert_eq!(summary.entries, vec![("👍".to_string(), 1)]);
}

#[test]
fn distinct_reactors_each_count_once_even_on_redelivery() {
    let store = MemEventStore::new();
    let handle = store.domain_open(NAMESPACE).unwrap();

    let a = reaction(&"a".repeat(64), ALICE, 100, TARGET, TARGET_AUTHOR, "👍");
    let bob = "bob-00000000000000000000000000000000000000000000000000000000000000";
    let b = reaction(&"b".repeat(64), bob, 100, TARGET, TARGET_AUTHOR, "👍");

    // Redeliver both several times.
    for _ in 0..3 {
        decode_and_route(&a, &handle).unwrap();
        decode_and_route(&b, &handle).unwrap();
    }

    let target = ReactionTarget::Event(TARGET.to_string());
    let summary = reaction_summary(&handle, &target).unwrap();
    assert_eq!(
        summary.total, 2,
        "two distinct reactors, redelivery is idempotent"
    );
    assert_eq!(summary.entries, vec![("👍".to_string(), 2)]);
}
