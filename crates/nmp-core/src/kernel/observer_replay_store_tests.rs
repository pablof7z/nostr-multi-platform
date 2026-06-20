//! NIT-1 regression tests for ADR-0062: store point-lookup for evicted event ids.
//!
//! These tests verify that `replay_read_cache_to_observer` serves events that
//! have been evicted from the in-RAM LRU cache (`Kernel::events`) but are still
//! present in the durable store, when those ids appear in the `event_ids` set of
//! the replay request's `InterestShape`s.
//!
//! Split into a sibling module to keep `observer_replay_tests` within the 500 LOC
//! ceiling (AGENTS.md § file-size rules).

use super::*;
use crate::actor::{
    new_event_observer_slot, register_rust_observer_muted, KernelEventObserver,
};
use crate::kernel::observer_replay::ObserverReplayRequest;
use crate::kernel::ram_eviction::EVENTS_RAM_HWM;
use crate::planner::InterestShape;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::KernelEvent;
use crate::subs::SubIdentity;
use std::sync::{Arc, Mutex};

// ─── Local test helpers (duplicated from observer_replay_tests — sibling   ────
//     modules cannot share private helpers across the Rust module boundary)  ────

struct CapturingObserver {
    events: Mutex<Vec<KernelEvent>>,
}

impl CapturingObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(Vec::new()),
        })
    }

    fn ids(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.id.clone())
            .collect()
    }
}

impl KernelEventObserver for CapturingObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

/// Ingest a minimal kind:1 event into the kernel.
fn ingest(kernel: &mut Kernel, id: &str, author: &str, created_at: u64, tags: Vec<Vec<String>>) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind: 1,
        tags,
        content: "test".into(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "test-relay",
        VerifiedEvent::from_raw_unchecked(raw),
    );
}

/// Build a simple author+kinds interest shape.
fn author_shape(author: &str, kinds: &[u32]) -> InterestShape {
    let k = kinds.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",");
    InterestShape::from_filter_json(&format!(r#"{{"kinds":[{k}],"authors":["{author}"]}}"#))
        .expect("valid author shape")
}

/// Build a SubIdentity for testing.
fn sub_identity(filter_json: &str, consumer_id: &str, scope: u32) -> SubIdentity {
    crate::subs::interest_builder::build_interest_pair(filter_json, consumer_id, scope)
        .map(|(id, _)| id)
        .expect("valid filter → identity")
}

/// Build a LogicalInterest (Tailing) for testing.
fn logical_interest(
    filter_json: &str,
    consumer_id: &str,
    scope: u32,
) -> crate::planner::LogicalInterest {
    crate::subs::interest_builder::build_interest_pair(filter_json, consumer_id, scope)
        .map(|(_, interest)| interest)
        .expect("valid filter → interest")
}

// ─── NIT-1 specific helpers ───────────────────────────────────────────────────

/// Build a valid 64-char hex string from a small integer suffix.
/// Produces IDs like "0000...0001" that are distinct from the root/reply IDs
/// used in the tests below (which use letter-prefixed repeating patterns).
fn filler_id(n: usize) -> String {
    format!("{:0>64x}", n + 1)
}

fn filler_author(n: usize) -> String {
    // Start at 0xdead to avoid colliding with numeric filler_id values.
    format!("{:0>64x}", 0xdead_usize + n)
}

/// Build an `{ids:[root_id]}` shape — the root-hydration complement used by the
/// Chirp thread feed alongside the `{#e:[root_id],kinds:[1,6]}` reply shape.
fn ids_shape(root_id: &str) -> InterestShape {
    InterestShape::from_filter_json(&format!(r#"{{"ids":["{root_id}"]}}"#))
        .expect("valid ids shape")
}

/// Build a `{#e:[root_id],kinds:[1,6]}` reply shape — the first shape of the
/// Chirp thread feed interest, covering replies/reposts referencing the root.
fn etag_kinds_shape(root_id: &str, kinds: &[u32]) -> InterestShape {
    let k = kinds.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",");
    // NOTE: cannot use r#"..."# here because "#" inside the string (in "#e")
    // would terminate the raw literal.  Use r##"..."## (two hashes) instead
    // so only "## closes the literal.
    InterestShape::from_filter_json(&format!(
        r##"{{"kinds":[{k}],"#e":["{root_id}"]}}"##
    ))
    .expect("valid #e+kinds shape")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// NIT-1 regression test — evicted thread root is served from the store.
///
/// This test MUST fail without the NIT-1 fix (root absent from replay because
/// it was evicted from the in-RAM LRU cache before the interest was opened)
/// and MUST pass with the fix (root served via `EventStore::get_by_id` through
/// the `{ids:[root_id]}` shape's `event_ids` point-lookup).
///
/// Scenario:
/// 1. Ingest root (oldest `created_at`) + a reply (very new `created_at`).
/// 2. Flood the RAM cache with `EVENTS_RAM_HWM` filler events (medium
///    `created_at`) so the total exceeds the HWM.
/// 3. Trigger `evict_ram_caches`: root is the oldest non-pinned event and gets
///    evicted; the reply (newest) stays in RAM.
/// 4. Verify root is NOT in `self.events` (precondition for the regression).
/// 5. Open a thread feed interest with two shapes: `{#e:[root_id],kinds:[1,6]}`
///    + `{ids:[root_id]}`.
/// 6. Assert the observer receives BOTH root (from store point-lookup) and
///    reply (from the RAM scan).
#[test]
fn evicted_root_served_from_store_via_event_ids() {
    let slot = new_event_observer_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot.clone());

    // Distinct, valid 64-char hex ids for root and reply.
    let root_id = "f0".repeat(32); // 64 chars, unique prefix
    let root_author = "aa".repeat(32);
    let reply_id = "e1".repeat(32);
    let reply_author = "bb".repeat(32);

    // Root has the OLDEST created_at — makes it the prime eviction candidate.
    ingest(&mut kernel, &root_id, &root_author, 1, vec![]);

    // Reply references the root via an e-tag.  Give it the NEWEST created_at
    // so it survives eviction (filler has medium timestamps between root and reply).
    let reply_tags = vec![vec!["e".to_string(), root_id.clone()]];
    ingest(
        &mut kernel,
        &reply_id,
        &reply_author,
        999_999_999,
        reply_tags,
    );

    // Flood the RAM cache with `EVENTS_RAM_HWM` additional events.
    // Medium `created_at` (100 + i) — newer than root but older than reply.
    // Total after flood: EVENTS_RAM_HWM + 2 (root + reply) entries.
    // Eviction removes the 2 oldest non-pinned entries: root (created_at=1)
    // and filler[0] (created_at=100).
    for i in 0..EVENTS_RAM_HWM {
        ingest(&mut kernel, &filler_id(i), &filler_author(i), 100 + i as u64, vec![]);
    }

    assert!(
        kernel.events.len() > EVENTS_RAM_HWM,
        "precondition: RAM cache must exceed HWM before eviction (len={})",
        kernel.events.len()
    );

    let report = kernel.evict_ram_caches();
    assert!(report.events_evicted > 0, "eviction must fire");

    // Precondition for the regression test: root MUST have been evicted.
    assert!(
        !kernel.events.contains_key(&root_id),
        "root must have been evicted from the RAM cache (it was the oldest non-pinned event)"
    );
    // Reply must still be in RAM (it has the highest created_at).
    assert!(
        kernel.events.contains_key(&reply_id),
        "reply must remain in the RAM cache after eviction"
    );

    // Open a thread feed interest with the two standard shapes:
    //   Shape 1: {#e:[root_id], kinds:[1,6]} — matches replies/reposts
    //   Shape 2: {ids:[root_id]}             — matches the root itself (NIT-1 path)
    let capturing = CapturingObserver::new();
    let observer_id = register_rust_observer_muted(&slot, capturing.clone());

    let root_filter = format!(r#"{{"ids":["{root_id}"]}}"#);
    let identity = sub_identity(&root_filter, "thread-nit1-consumer", 77);
    let interest = logical_interest(&root_filter, "thread-nit1-consumer", 77);

    let replay = ObserverReplayRequest {
        observer_id,
        shapes: vec![
            etag_kinds_shape(&root_id, &[1, 6]),
            ids_shape(&root_id),
        ],
        limit: 80,
    };
    kernel.open_interest_with_observer_replay(identity, interest, replay, "test-thread-nit1");

    let ids = capturing.ids();
    // Root must be present — served from the store via `{ids:[root_id]}` point-lookup.
    assert!(
        ids.contains(&root_id),
        "evicted root MUST be served from the store via the event_ids point-lookup; \
         got ids: {ids:?}"
    );
    // Reply must be present — served from the RAM cache via the `#e` shape.
    assert!(
        ids.contains(&reply_id),
        "reply MUST be served from the RAM cache via the #e shape; got ids: {ids:?}"
    );
}

/// No double-delivery when root is in RAM AND listed in `{ids:[root_id]}`.
///
/// An event whose id appears in `InterestShape.event_ids` AND that is matched
/// by the RAM scan (it is still in `self.events`) must be delivered exactly
/// once — the store point-lookup skips ids already present in the RAM cache.
#[test]
fn no_double_delivery_root_in_ram_and_event_ids() {
    let slot = new_event_observer_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot.clone());

    let root_id = "c3".repeat(32);
    let root_author = "dd".repeat(32);

    // Ingest root — goes into both `self.events` (RAM) and `self.store`.
    // No eviction: root remains in the RAM cache.
    ingest(&mut kernel, &root_id, &root_author, 5_000, vec![]);
    assert!(
        kernel.events.contains_key(&root_id),
        "precondition: root must be in the RAM cache"
    );

    // Open interest with two shapes where BOTH can match the root:
    //   Shape 1: author+kinds — RAM scan picks up the root.
    //   Shape 2: {ids:[root_id]} — event_ids path would also find it.
    // The NIT-1 fix deduplicates via `!self.events.contains_key(hex_id)` so the
    // store is NOT queried for an id already in RAM, ensuring single delivery.
    let capturing = CapturingObserver::new();
    let observer_id = register_rust_observer_muted(&slot, capturing.clone());

    let root_filter = format!(r#"{{"ids":["{root_id}"]}}"#);
    let identity = sub_identity(&root_filter, "dedup-nit1-consumer", 88);
    let interest = logical_interest(&root_filter, "dedup-nit1-consumer", 88);

    let replay = ObserverReplayRequest {
        observer_id,
        shapes: vec![
            author_shape(&root_author, &[1]),
            ids_shape(&root_id),
        ],
        limit: 80,
    };
    kernel.open_interest_with_observer_replay(identity, interest, replay, "test-dedup-nit1");

    let ids = capturing.ids();
    let root_occurrences = ids.iter().filter(|id| *id == &root_id).count();
    assert_eq!(
        root_occurrences,
        1,
        "root that is in RAM and in event_ids MUST be delivered exactly once, \
         not {root_occurrences} times; got ids: {ids:?}"
    );
}
