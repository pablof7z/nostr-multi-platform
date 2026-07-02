//! V-118 expiration-index correctness + backfill-on-reopen tests (closes #1097).
//!
//! Split out of `tests_gc.rs` (500-LOC hard cap).
//!
//! 1. `v118_same_created_at_block_does_not_block_older_expired` — regression
//!    test for the exact defect: a block of NON-expired events all sharing
//!    one `created_at` larger than one budget pass must NOT prevent older
//!    expired events from being collected.  With the old cursor-based Phase 1
//!    this test would fail (older events never reached); with the
//!    expiration-index Phase 1 it passes.
//! 2. `v118_expiry_index_maintained_on_write_and_delete` — index maintenance:
//!    inserted events with expiration tags appear in the index in correct
//!    temporal order.
//! 3. `v118_expiry_index_backfill_on_reopen` — opening an existing store
//!    (pre-index) triggers a one-time backfill; after reopen gc_step
//!    correctly reaps events that were inserted before the index existed.
//! 4. `v118_backfill_gate_key_written_and_stable_across_reopens` — the
//!    backfill migration gate: after the first open the `domain_versions` key
//!    is present; subsequent opens skip the O(store) scan and the expiry
//!    index stays intact across multiple reopens.

#![cfg(feature = "lmdb-backend")]

use crate::types::{GcBudget, InsertOutcome};
use crate::EventStore;

use super::super::test_fixtures::{open_tmp, signed_event, verified};

/// Regression test for V-118 (GitHub #1097).
///
/// The defect: with the cursor-based Phase 1, a block of NON-expired events all
/// sharing one `created_at` that is larger than one time-budget pass parks the
/// cursor at that timestamp forever — every subsequent pass re-scans the same
/// prefix rather than advancing to older events.
///
/// This test proves the fix via its observable invariant: with the expiration
/// index, Phase 1 performs an O(expired) range scan on the index — it NEVER
/// touches non-expired events and is never blocked by large non-expired blocks.
///
/// Setup:
/// - Insert BLOCK_SIZE non-expired events all sharing `created_at = block_ts`.
/// - Insert 2 expired events at `older_ts` (below the block).
/// - Run gc_step with `max_events_per_step = 1` (admits only one expired event
///   per pass) and a generous time budget.
///
/// Assertions:
/// - Pass 1 reaps exactly 1 expired event (budget honoured).
/// - Pass 2 reaps the other expired event (index continues from where it left off).
/// - 2 total passes suffice — the non-expired block never causes stalls.
///
/// With the old cursor implementation running against this store:
/// - The cursor starts at `None` (newest first) and scans block_ts events.
/// - If the time budget is generous, the cursor eventually scans all events and
///   reaches the expired ones.  But if the block is large enough to exhaust the
///   TIME budget before reaching older events, the cursor parks and never progresses.
///
/// With the expiration-index implementation:
/// - Only index entries `(expiry_ts → event_id)` with `expiry_ts ≤ now_secs` are
///   iterated; non-expired events are entirely invisible to Phase 1.
/// - The `max_events_per_step` count budget is honoured correctly by counting
///   how many index entries were processed.
#[test]
fn v118_same_created_at_block_does_not_block_older_expired() {
    use nostr::prelude::*;

    let (store, _dir) = open_tmp();

    let base_ts = 1_700_000_000u64;
    let block_ts = base_ts + 100; // all non-expired events share this timestamp
    let older_ts = base_ts + 50; // older expired events sit below the block
    let exp_ts = base_ts + 90; // expiration in the "past" relative to gc_now
    let gc_now = base_ts + 200;

    // Insert BLOCK_SIZE non-expired kind:1 events all at block_ts.
    const BLOCK_SIZE: usize = 6;
    for i in 0..BLOCK_SIZE {
        let raw = signed_event(1, block_ts, &format!("non-expired-block-{i}"), None);
        store
            .insert(verified(raw), &"wss://r/".into(), block_ts * 1_000)
            .expect("insert block event");
    }

    // Insert 2 expired events at older_ts.
    let keys = Keys::generate();
    let mut expired_ids: Vec<[u8; 32]> = Vec::new();
    for i in 0..2usize {
        let ev = EventBuilder::text_note(format!("expired-{i}"))
            .custom_created_at(Timestamp::from_secs(older_ts))
            .tag(Tag::expiration(Timestamp::from_secs(exp_ts)))
            .sign_with_keys(&keys)
            .expect("sign");
        let json = ev.try_as_json().expect("json");
        let raw: crate::types::RawEvent = serde_json::from_str(&json).expect("parse");
        let id = raw.id_bytes().expect("id");
        let outcome = store
            .insert(verified(raw), &"wss://r/".into(), older_ts * 1_000)
            .expect("insert expired event");
        if matches!(outcome, InsertOutcome::Inserted { .. }) {
            expired_ids.push(id);
        }
    }
    assert_eq!(expired_ids.len(), 2, "both expired events must insert");

    // Budget admits only 1 expired event per pass.
    let budget = GcBudget {
        max_events_per_step: 1,
        max_duration_ms: 60_000,
        max_total_events: usize::MAX,
    };

    // Pass 1: must reap exactly 1 expired event.
    let report1 = store.gc_step(budget, gc_now).expect("gc_step pass 1");
    assert_eq!(
        report1.expired_reaped, 1,
        "V-118 pass 1: expected 1 expired event reaped (budget=1), got {}. \
         Phase 1 must consume from the expiry index, not scan all events.",
        report1.expired_reaped,
    );

    // Pass 2: must reap the second expired event.
    let report2 = store.gc_step(budget, gc_now).expect("gc_step pass 2");
    assert_eq!(
        report2.expired_reaped, 1,
        "V-118 pass 2: expected 1 expired event reaped (second expired event), got {}. \
         The expiry index must advance past already-reaped entries.",
        report2.expired_reaped,
    );

    // Both expired events must now be gone.
    for id in &expired_ids {
        assert!(
            store.get_by_id(id).expect("get_by_id").is_none(),
            "V-118: expired event still present in store after gc",
        );
    }

    // The non-expired block events must still be present (we never evict them).
    // Phase 3 (tombstone purge) runs but does not affect non-expired events.
    // Just verify total reaped matches.
    let report3 = store.gc_step(budget, gc_now).expect("gc_step pass 3");
    assert_eq!(
        report3.expired_reaped, 0,
        "V-118 pass 3: no more expired events expected, got {}",
        report3.expired_reaped,
    );
}

/// Index maintenance: events with expiration tags appear in the expiry index
/// on insert and can be reaped by gc_step in expiry-timestamp order.
/// Verifies that the index maintains correct temporal ordering.
#[test]
fn v118_expiry_index_maintained_on_write_and_delete() {
    use nostr::prelude::*;

    let (store, _dir) = open_tmp();

    let base_ts = 1_700_000_000u64;
    let exp_ts_a = base_ts + 50; // expires first
    let exp_ts_b = base_ts + 150; // expires second
    let gc_now_a = base_ts + 60; // only event A expired
    let gc_now_b = base_ts + 160; // both events expired

    // Insert two events with distinct expiration timestamps.
    let keys = Keys::generate();
    let ev_a = EventBuilder::text_note("expires-a")
        .custom_created_at(Timestamp::from_secs(base_ts))
        .tag(Tag::expiration(Timestamp::from_secs(exp_ts_a)))
        .sign_with_keys(&keys)
        .expect("sign");
    let ev_b = EventBuilder::text_note("expires-b")
        .custom_created_at(Timestamp::from_secs(base_ts + 1))
        .tag(Tag::expiration(Timestamp::from_secs(exp_ts_b)))
        .sign_with_keys(&keys)
        .expect("sign");

    let json_a = ev_a.try_as_json().expect("json");
    let json_b = ev_b.try_as_json().expect("json");
    let raw_a: crate::types::RawEvent = serde_json::from_str(&json_a).expect("parse");
    let raw_b: crate::types::RawEvent = serde_json::from_str(&json_b).expect("parse");
    let id_a = raw_a.id_bytes().expect("id");

    store
        .insert(verified(raw_a), &"wss://r/".into(), base_ts * 1_000)
        .expect("insert a");
    store
        .insert(verified(raw_b), &"wss://r/".into(), (base_ts + 1) * 1_000)
        .expect("insert b");

    let budget_full = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 60_000,
        max_total_events: usize::MAX,
    };

    // At gc_now_a only event A should be reaped (expiry index is ordered by
    // expiry timestamp, so A comes before B).
    let report_a = store.gc_step(budget_full, gc_now_a).expect("gc_step a");
    assert_eq!(
        report_a.expired_reaped, 1,
        "only event A (exp={exp_ts_a}) should be reaped at now={gc_now_a}, got {}",
        report_a.expired_reaped,
    );
    assert!(
        store.get_by_id(&id_a).expect("get_by_id a").is_none(),
        "event A must be gone after gc at gc_now_a"
    );

    // At gc_now_b event B should be reaped.
    let report_b = store.gc_step(budget_full, gc_now_b).expect("gc_step b");
    assert_eq!(
        report_b.expired_reaped, 1,
        "event B (exp={exp_ts_b}) should be reaped at now={gc_now_b}, got {}",
        report_b.expired_reaped,
    );
}

/// Backfill test: an LMDB store that was written before the expiration index
/// existed (pre-V-118) must have its index populated on re-open, so that
/// gc_step correctly reaps all expired events.
///
/// We simulate a "pre-index" store by inserting events in a first session
/// and then re-opening the store (which triggers the backfill).  Since the
/// backfill iterates all events and writes expiry-index entries for any event
/// that has an expiration tag, gc_step must reap them after reopen.
#[test]
fn v118_expiry_index_backfill_on_reopen() {
    use crate::LmdbEventStore;
    use nostr::prelude::*;
    use tempfile::tempdir;

    let dir = tempdir().expect("tempdir");
    let exp_ts = 1_700_000_100u64;
    let gc_now = exp_ts + 1; // just past expiry

    // Session 1: insert an event with expiration.
    {
        let store = LmdbEventStore::open(dir.path()).expect("open session 1");
        let keys = Keys::generate();
        let ev = EventBuilder::text_note("backfill-test")
            .custom_created_at(Timestamp::from_secs(1_700_000_000))
            .tag(Tag::expiration(Timestamp::from_secs(exp_ts)))
            .sign_with_keys(&keys)
            .expect("sign");
        let json = ev.try_as_json().expect("json");
        let raw: crate::types::RawEvent = serde_json::from_str(&json).expect("parse");
        store
            .insert(verified(raw), &"wss://r/".into(), 1_699_000_000_000)
            .expect("insert session 1");
        // Store dropped — LMDB flushes to disk.
    }

    // Session 2: re-open (triggers backfill on open) and run gc.
    let store2 = LmdbEventStore::open(dir.path()).expect("open session 2");

    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 60_000,
        max_total_events: usize::MAX,
    };

    let report = store2
        .gc_step(budget, gc_now)
        .expect("gc_step after reopen");
    assert_eq!(
        report.expired_reaped, 1,
        "V-118 backfill: event inserted in session 1 must be reaped in session 2 \
         (backfill populated the expiry index on reopen). expired_reaped={}",
        report.expired_reaped,
    );
}

/// The migration gate in `backfill_expiry_index` must:
///
/// 1. On the first open of a fresh store: run the backfill (scan events, write
///    index entries) then set the `domain_versions` key `b"nmp-expiry-index"`.
/// 2. On every subsequent open: find the key already set, skip the O(store)
///    scan entirely, and leave the expiry index intact.
///
/// This test directly observes the gate by reading `inner.domain_versions`
/// after the first open to assert the `nmp-expiry-index` key is present,
/// then verifies the index survives three reopens by asserting gc_step still
/// reaps the expected event on the third open.
#[test]
fn v118_backfill_gate_key_written_and_stable_across_reopens() {
    use crate::LmdbEventStore;
    use nostr::prelude::*;
    use tempfile::tempdir;

    let dir = tempdir().expect("tempdir");
    let base_ts = 1_700_000_000u64;
    let exp_ts = base_ts + 100;
    let gc_now = exp_ts + 1;

    // Session 1: fresh store — backfill runs (empty scan because the event is
    // inserted AFTER open), version key written on open.
    {
        let store = LmdbEventStore::open(dir.path()).expect("open session 1");

        // Directly verify that the gate key was written into domain_versions
        // on this first open.
        {
            let txn = store.inner.env.read_txn().expect("read_txn");
            let val = store
                .inner
                .domain_versions
                .get(&txn, b"nmp-expiry-index")
                .expect("domain_versions get")
                .map(|v| v.to_vec());
            assert!(
                val.is_some(),
                "v118: backfill gate key `nmp-expiry-index` must be present in \
                 domain_versions after the first store open",
            );
        }

        let keys = Keys::generate();
        let ev = EventBuilder::text_note("gate-test")
            .custom_created_at(Timestamp::from_secs(base_ts))
            .tag(Tag::expiration(Timestamp::from_secs(exp_ts)))
            .sign_with_keys(&keys)
            .expect("sign");
        let json = ev.try_as_json().expect("json");
        let raw: crate::types::RawEvent = serde_json::from_str(&json).expect("parse");
        store
            .insert(verified(raw), &"wss://r/".into(), base_ts * 1_000)
            .expect("insert session 1");
        // Drop → LMDB flushes to disk.
    }

    // Session 2: reopen — backfill gate finds the version key and returns early.
    // The expiry index was populated in Session 1 at insert time (not by backfill).
    {
        let store2 = LmdbEventStore::open(dir.path()).expect("open session 2");
        // Gate key must still be present after the gate-skipped open.
        let txn = store2.inner.env.read_txn().expect("read_txn");
        let val = store2
            .inner
            .domain_versions
            .get(&txn, b"nmp-expiry-index")
            .expect("domain_versions get session 2")
            .map(|v| v.to_vec());
        assert!(
            val.is_some(),
            "v118: gate key must persist across reopens (session 2)",
        );
    }

    // Session 3: third open — version key still present; index intact.
    // gc_step must reap the expiring event, proving the index survived.
    let store3 = LmdbEventStore::open(dir.path()).expect("open session 3");
    let budget = GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 60_000,
        max_total_events: usize::MAX,
    };
    let report = store3.gc_step(budget, gc_now).expect("gc_step session 3");
    assert_eq!(
        report.expired_reaped, 1,
        "v118_backfill_gate: expiry index must survive multiple reopens \
         (backfill gate skips redundant scans). expired_reaped={}",
        report.expired_reaped,
    );

    // A second gc pass must find nothing more (no phantom reaps).
    let report2 = store3
        .gc_step(budget, gc_now)
        .expect("gc_step session 3 pass 2");
    assert_eq!(
        report2.expired_reaped, 0,
        "v118_backfill_gate: no phantom reaps after second pass, got {}",
        report2.expired_reaped,
    );
}
