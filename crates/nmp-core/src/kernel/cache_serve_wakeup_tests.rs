//! Tests for event-driven cache-serve wakeups (#1520).
//!
//! Verifies that `note_store_mutation` + `drain_cache_serve_wakeups` re-arm
//! already-served interests when a matching live event arrives, coalesce
//! multiple rapid inserts to a single re-arm per actor turn, and correctly
//! ignore interests that are not yet fully served or have been withdrawn.

use super::*;
use crate::actor::{new_event_observer_slot, register_rust_observer, KernelEventObserver};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::KernelEvent;
use nmp_network::role::RelayRole;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use super::cache_serve_tests::{drain_cache_serves, hex_pk, signed_note};
use super::interest_install_cache_serve_support::{
    author_kind1_interest, kp_interest, seed_kp_event, sub_id,
};
use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};

struct CountingObserver {
    count: AtomicU32,
}

impl CountingObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            count: AtomicU32::new(0),
        })
    }

    fn count(&self) -> u32 {
        self.count.load(Ordering::SeqCst)
    }
}

impl KernelEventObserver for CountingObserver {
    fn on_kernel_event(&self, _event: &KernelEvent) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

// ─── 1. Wakeup on insert ─────────────────────────────────────────────────────

/// Register interest → drain initial serve → assert completion key present →
/// insert matching event → call note_store_mutation → run_cache_serve_step →
/// assert the new event appears in the events cache (i.e. cache-serve ran for
/// it) and the completion key is re-recorded.
#[test]
fn wake_registration_before_insert() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(hex_pk("aa"));

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // Register a kind:1 interest for this author.
    let interest = author_kind1_interest(1, &author);
    let shape = interest.shape.clone();
    let identity = sub_id(1);
    let key = identity.key;
    kernel.register_interest(
        &[InterestRegistration {
            identity,
            interest,
            policy: InterestWrite::EnsureAbsent,
        }],
        "test",
    );
    // Drain any enqueued serves (store is empty, so this finishes immediately).
    drain_cache_serves(&mut kernel, 4);

    // The completion key for this interest must be in served_interest_shapes.
    let ckey = crate::kernel::cache_serve::completion_key_for_interest(&key, &shape);
    assert!(
        kernel.served_interest_shapes.contains(&ckey),
        "completion key must be in served_interest_shapes after initial serve"
    );
    assert!(
        kernel.store_wakeups.cache_serve.is_empty(),
        "no wakeups before any insert"
    );

    // Now insert a matching event via the live ingest path.
    let ev = signed_note(&keys, "wake test", 1_700_000_001);
    let ev_id = ev.id.clone();
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.test/", "test-sub", ev);

    // The ingest path fires note_store_mutation, which should have armed a wakeup.
    assert!(
        kernel.store_wakeups.cache_serve.contains(&ckey),
        "note_store_mutation must arm a wakeup for the already-served interest"
    );

    // Simulate one actor tick: run_cache_serve_step drains wakeups first.
    kernel.run_cache_serve_step();

    // The completion key must be back in served_interest_shapes.
    assert!(
        kernel.served_interest_shapes.contains(&ckey),
        "completion key must be re-recorded after wakeup serve"
    );
    // Wakeup buffer must be drained.
    assert!(
        kernel.store_wakeups.cache_serve.is_empty(),
        "wakeup buffer must be empty after run_cache_serve_step"
    );
    // The event must be visible in the read cache (served from the store).
    assert!(
        kernel.events.contains_key(ev_id.as_str()),
        "newly inserted event must be served from store after wakeup: {ev_id}"
    );
}

/// Issue #1575 regression: an already-served non-timeline interest whose
/// first matching event arrives live must not double-notify observers when a
/// wakeup re-serve scans the store.
#[test]
fn first_inserted_non_timeline_event_dedups_wakeup_reserve() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let receiver_keys = ::nostr::Keys::generate();
    let receiver_hex = receiver_keys.public_key().to_hex();
    let publisher_keys = ::nostr::Keys::generate();
    let base_ts: u64 = 1_780_000_000;

    let slot = new_event_observer_slot();
    let observer = CountingObserver::new();
    register_rust_observer(&slot, observer.clone());
    kernel.set_event_observers_handle(slot);
    kernel.active_account = Some(receiver_hex.clone());

    let interest = kp_interest(1_575, &receiver_hex);
    let shape = interest.shape.clone();
    let identity = sub_id(1_575);
    let key = identity.key;
    kernel.register_interest(
        &[InterestRegistration {
            identity,
            interest,
            policy: InterestWrite::EnsureAbsent,
        }],
        "test",
    );
    drain_cache_serves(&mut kernel, 4);
    let ckey = crate::kernel::cache_serve::completion_key_for_interest(&key, &shape);
    assert!(kernel.served_interest_shapes.contains(&ckey));
    assert_eq!(observer.count(), 0, "empty initial serve must not notify");

    let event_id = seed_kp_event(&mut kernel, &publisher_keys, &receiver_hex, base_ts);
    assert_eq!(observer.count(), 1, "live Inserted event must notify once");
    assert!(
        kernel.events.contains_key(event_id.as_str()),
        "live non-timeline event matching an active interest must enter the \
         projection cache so cache-serve can dedup it"
    );
    assert!(
        kernel.store_wakeups.cache_serve.contains(&ckey),
        "live insert must arm the served-interest wakeup"
    );

    kernel.run_cache_serve_step();

    assert_eq!(
        observer.count(),
        1,
        "wakeup re-serve must not double-notify an already observed event"
    );
    assert!(
        kernel.served_interest_shapes.contains(&ckey),
        "completion key must be restored after the wakeup serve drains"
    );
}

// ─── 2. Insert before registration ───────────────────────────────────────────

/// Insert event first → register interest → assert register-time serve delivers
/// it (no double-delivery). The wakeup mechanism must not fire for interests
/// not yet served.
#[test]
fn insert_before_registration() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(hex_pk("aa"));

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // Insert event BEFORE registering any interest.
    let ev = signed_note(&keys, "early event", 1_700_000_000);
    let ev_id = ev.id.clone();
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.test/", "test-sub", ev);

    // No wakeups yet — nothing is in served_interest_shapes.
    assert!(
        kernel.store_wakeups.cache_serve.is_empty(),
        "no wakeup must be armed before any interest is registered"
    );

    // Now register the interest.
    let interest = author_kind1_interest(1, &author);
    let identity = sub_id(1);
    kernel.register_interest(
        &[InterestRegistration {
            identity,
            interest,
            policy: InterestWrite::EnsureAbsent,
        }],
        "test",
    );
    drain_cache_serves(&mut kernel, 4);

    // The event must be served exactly once from the store.
    assert!(
        kernel.events.contains_key(ev_id.as_str()),
        "event inserted before registration must be served by register-time cache-serve"
    );
    // No duplicate: events cache has exactly one copy.
    assert_eq!(
        kernel.events.len(),
        1,
        "event must appear exactly once (no double-delivery)"
    );
}

// ─── 3. Closed view release drops wakeups ────────────────────────────────────

/// Register → serve → arm wakeup → drop_owner / withdraw interest →
/// drain_cache_serve_wakeups → assert no serve re-enqueued for the dead
/// interest.
#[test]
fn closed_view_release_drops_wakeups() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(hex_pk("aa"));

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let interest = author_kind1_interest(1, &author);
    let shape = interest.shape.clone();
    let identity = sub_id(1);
    let key = identity.key;
    kernel.register_interest(
        &[InterestRegistration {
            identity: identity.clone(),
            interest,
            policy: InterestWrite::EnsureAbsent,
        }],
        "test",
    );
    drain_cache_serves(&mut kernel, 4);

    let ckey = crate::kernel::cache_serve::completion_key_for_interest(&key, &shape);
    assert!(kernel.served_interest_shapes.contains(&ckey));

    // Insert a matching event to arm the wakeup.
    let ev = signed_note(&keys, "wakeup then close", 1_700_000_001);
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.test/", "test-sub", ev);
    assert!(
        kernel.store_wakeups.cache_serve.contains(&ckey),
        "wakeup must be armed"
    );

    // Drop the owner (simulates view close). This is the sole owner, so the
    // slot (and its live interest) must be removed.
    let removed = kernel.lifecycle.registry_mut().drop_owner(&identity);
    assert!(
        removed,
        "dropping the sole owner must remove the interest slot"
    );

    // Now drain wakeups. The interest is no longer in the registry, so
    // drain_cache_serve_wakeups must silently skip re-enqueue.
    kernel.drain_cache_serve_wakeups();

    assert!(
        kernel.store_wakeups.cache_serve.is_empty(),
        "wakeup buffer must be drained"
    );
    // pending_cache_serves must not have gained a new entry for the dead interest.
    let dead_in_queue = kernel
        .pending_cache_serves
        .iter()
        .any(|p| p.completion_key == ckey);
    assert!(
        !dead_in_queue,
        "a dropped interest must not be re-enqueued after wakeup"
    );
}

// ─── 4. Coalesce many inserts to one re-arm ──────────────────────────────────

/// N inserts in one actor turn without an intervening run_cache_serve_step →
/// store_wakeups.cache_serve must have ≤1 entry per interest (BTreeSet coalesces).
#[test]
fn coalesce_many_inserts_one_rearm() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(hex_pk("aa"));

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let interest = author_kind1_interest(1, &author);
    let shape = interest.shape.clone();
    let identity = sub_id(1);
    let key = identity.key;
    kernel.register_interest(
        &[InterestRegistration {
            identity,
            interest,
            policy: InterestWrite::EnsureAbsent,
        }],
        "test",
    );
    drain_cache_serves(&mut kernel, 4);

    let ckey = crate::kernel::cache_serve::completion_key_for_interest(&key, &shape);
    assert!(kernel.served_interest_shapes.contains(&ckey));

    // Insert 5 matching events without draining wakeups in between.
    for i in 0..5u64 {
        let ev = signed_note(&keys, &format!("burst event {i}"), 1_700_000_100 + i);
        kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.test/", "test-sub", ev);
    }

    // The wakeup set must contain exactly one entry for this interest.
    assert_eq!(
        kernel.store_wakeups.cache_serve.len(),
        1,
        "5 rapid inserts for the same interest must coalesce to exactly 1 wakeup entry"
    );
    assert!(kernel.store_wakeups.cache_serve.contains(&ckey));
}

// ─── 5. Mid-replay insert does not duplicate the pending serve ────────────────

/// Register interest → DO NOT drain serves (interest still in pending queue) →
/// insert matching event → assert pending serve is not duplicated.
///
/// Rationale: `note_store_mutation` only arms wakeups for interests already in
/// `served_interest_shapes`. An interest still pending in the continuation
/// queue is not yet served, so it must not gain an extra wakeup entry.
#[test]
fn replay_chunk_no_wakeup_reenqueue() {
    // Tiny visible_limit so the budget runs out and interests stay pending.
    let mut kernel = Kernel::new(1);
    kernel.active_account = Some(hex_pk("aa"));

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let base_ts: u64 = 1_700_000_000;

    // Seed 5 events into the store first (before registering the interest).
    kernel.timeline_authors.insert(author.clone());
    for i in 0..5u64 {
        let ev = signed_note(&keys, &format!("pre-seed {i}"), base_ts + i);
        kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.test/", "seed-sub", ev);
    }

    // Clear in-memory caches (cold restart sim) but keep store.
    kernel.events.clear();
    kernel.timeline.clear();
    kernel.clear_served_interest_shapes();

    // Register the interest — this enqueues a cache-serve but does NOT complete
    // it in full because visible_limit=1 means the budget is tiny.
    let interest = author_kind1_interest(1, &author);
    let shape = interest.shape.clone();
    let identity = sub_id(1);
    let key = identity.key;
    kernel.register_interest(
        &[InterestRegistration {
            identity,
            interest,
            policy: InterestWrite::EnsureAbsent,
        }],
        "test",
    );

    // Run exactly one step (may complete or not — depends on store size vs budget).
    // The key point: if the interest is still pending (not in served_interest_shapes),
    // a live insert must NOT arm a wakeup.
    let ckey = crate::kernel::cache_serve::completion_key_for_interest(&key, &shape);
    let already_served = kernel.served_interest_shapes.contains(&ckey);

    let queue_depth_before = kernel.pending_cache_serves.len();

    // Insert a new matching event — must NOT duplicate the pending serve entry.
    let new_ev = signed_note(&keys, "live event during replay", base_ts + 100);
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.test/", "live-sub", new_ev);

    if !already_served {
        // Interest still pending: no wakeup must be armed.
        assert!(
            !kernel.store_wakeups.cache_serve.contains(&ckey),
            "a still-pending interest must not gain a wakeup on insert"
        );
        // Pending queue must not have gained duplicate entries.
        let queue_depth_after = kernel.pending_cache_serves.len();
        assert_eq!(
            queue_depth_after, queue_depth_before,
            "pending serves must not be duplicated by a live insert during replay"
        );
    }
    // (If it was already served in the one-step drain above, the wakeup is
    // legitimate — that's the normal wake_registration_before_insert path.)
}
