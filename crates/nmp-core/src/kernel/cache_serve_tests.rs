//! ADR-0045 E1 — store-cache serve acceptance tests.
//!
//! These tests verify the four ADR-0045 E1 invariants:
//!
//! 1. **Universal acceptance** — on second launch (in-memory caches cleared,
//!    store warm), `sync_follow_feed_interests` drives cache-serve and
//!    re-populates `events` and `timeline` from the store without any relay
//!    connectivity.
//!
//! 2. **Budget-bounded serve** — a store with `CACHE_SERVE_BUDGET_EVENTS + 1`
//!    events does not stall the actor; at most `CACHE_SERVE_BUDGET_EVENTS` are
//!    served in one call (budget cap).
//!
//! 3. **Dedup-on-redelivery** — events already in the `events` cache (from a
//!    prior relay deliver) are skipped on cache-serve; the cache is not
//!    double-populated.
//!
//! 4. **Watermark ⇄ serve invariant** — the `StoreQuery` variants produced by
//!    `shape_to_store_queries` for E1 shapes are exactly the shapes the
//!    watermark rewrite covers (structural identity, not a runtime assertion).
//!
//! 5. **Completion-key one-shot** — calling `sync_follow_feed_interests` a
//!    second time for the same follow set is a no-op (the completion key is
//!    already in `served_interest_shapes`).
//!
//! 6. **Account-switch clears completion set** — after
//!    `reconcile_follow_feed_after_identity_change`, the completion set is
//!    empty and a fresh serve runs for the new account's interests.

use super::*;
use crate::kernel::cache_serve::{
    shape_to_store_queries, CACHE_SERVE_BUDGET_EVENTS,
};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::StoreQuery;
use std::collections::BTreeSet;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Build a 64-char lowercase hex string by repeating `prefix` until 64 chars.
fn hex_pk(prefix: &str) -> String {
    let padded: String = prefix
        .chars()
        .chain(std::iter::repeat('0'))
        .take(64)
        .collect();
    padded
}

/// Signed kind:1 event helper — mirrors `ingest_tests::signed_note`.
fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Timestamp};
    let ev = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev.tags.iter().map(|t: &::nostr::Tag| t.as_slice().to_vec()).collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

/// Seed `n` signed kind:1 events from `keys` into `kernel` by calling
/// `ingest_timeline_event` (uses the real store insert path). The author must
/// already be in `kernel.timeline_authors`. Returns the ids in order.
fn seed_events(
    kernel: &mut Kernel,
    keys: &::nostr::Keys,
    n: usize,
    base_ts: u64,
) -> Vec<String> {
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        let ev = signed_note(keys, &format!("seed event {i}"), base_ts + i as u64);
        ids.push(ev.id.clone());
        kernel.ingest_timeline_event(
            RelayRole::Content,
            "wss://seed.relay/",
            "follow-feed-default",
            ev,
        );
    }
    ids
}

/// Clear `kernel.events` and `kernel.timeline` to simulate a cold second
/// launch (store persisted, in-memory caches empty).
fn simulate_cold_restart(kernel: &mut Kernel) {
    kernel.events.clear();
    kernel.timeline.clear();
    kernel.metric_stored_events = 0;
    kernel.metric_note_events = 0;
    // Clear the served-interest completion set so the next
    // `sync_follow_feed_interests` triggers a fresh cache-serve.
    kernel.served_interest_shapes.clear();
}

// ─── 1. Universal acceptance ─────────────────────────────────────────────────

/// D1 / ADR-0045 E1 core acceptance:
///
/// 1. Seed events into the store via the live ingest path.
/// 2. Simulate a cold second launch by clearing in-memory caches.
/// 3. Re-open the follow-feed interest via `sync_follow_feed_interests`.
/// 4. Assert events reappear in `kernel.events` and `kernel.timeline` without
///    any relay connectivity.
///
/// This is the central falsifiability probe: if `cache_serve_for_interest` is
/// broken or not called, `kernel.events` will be empty and the test fails.
#[test]
fn e1_stored_events_reappear_after_cold_restart_without_relay() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let base_ts: u64 = 1_700_000_000;

    // Set up follow-feed: the author is followed.
    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([1u32]);
    kernel.timeline_authors.insert(author.clone());

    // Phase 1: seed 3 events into the live kernel (store + in-memory caches).
    let ids = seed_events(&mut kernel, &keys, 3, base_ts);
    assert_eq!(kernel.events.len(), 3, "all seeded events must be in events cache");

    // Phase 2: cold restart — clear in-memory caches, keep the store warm.
    simulate_cold_restart(&mut kernel);
    assert!(kernel.events.is_empty(), "events cache must be empty after restart");
    assert!(kernel.timeline.is_empty(), "timeline must be empty after restart");

    // Phase 3: re-open the follow-feed interest (triggers cache-serve).
    kernel.sync_follow_feed_interests(&[author.clone()]);

    // Phase 4: verify all seeded events are back.
    for id in &ids {
        assert!(
            kernel.events.contains_key(id.as_str()),
            "E1: event {id} must be served from the store after cold restart"
        );
    }
    assert!(
        kernel.timeline.iter().any(|id| ids.contains(id)),
        "E1: at least one seeded event must appear in the timeline after cache-serve"
    );
}

// ─── 2. Budget-bounded serve ─────────────────────────────────────────────────

/// ADR-0045 budget discipline: no actor stall on a large store.
///
/// Seeds `CACHE_SERVE_BUDGET_EVENTS + 5` events from one author, then
/// triggers cache-serve and asserts the served count is at most the budget.
///
/// This does NOT assert a minimum — if the store returns fewer than the budget
/// that is fine. The assertion is the upper bound: the actor is never asked to
/// process more than `CACHE_SERVE_BUDGET_EVENTS` events per interest.
#[test]
fn e1_serve_is_bounded_by_budget() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let base_ts: u64 = 1_700_000_000;

    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([1u32]);
    kernel.timeline_authors.insert(author.clone());

    let over_budget = CACHE_SERVE_BUDGET_EVENTS + 5;
    seed_events(&mut kernel, &keys, over_budget, base_ts);
    assert_eq!(kernel.events.len(), over_budget, "all seeded events must be in events cache");

    simulate_cold_restart(&mut kernel);
    assert!(kernel.events.is_empty(), "events cache must be empty after restart");

    // cache-serve fires via sync_follow_feed_interests.
    kernel.sync_follow_feed_interests(&[author.clone()]);

    let served = kernel.events.len();
    assert!(
        served <= CACHE_SERVE_BUDGET_EVENTS,
        "E1 budget: served {served} events but budget is {CACHE_SERVE_BUDGET_EVENTS}"
    );
    // Also: the serve did actually serve some events (> 0).
    assert!(
        served > 0,
        "E1 budget: at least one event must be served from a warm store"
    );
}

// ─── 3. Dedup-on-redelivery ───────────────────────────────────────────────────

/// Events already in `kernel.events` (from a live relay deliver that ran
/// BEFORE the interest was also served from the store) must NOT be
/// double-inserted.
///
/// The test pre-populates `kernel.events` manually to simulate a relay
/// delivering an event just before the cache-serve fires, then asserts
/// that `kernel.events.len()` does not grow for events that were already
/// in the cache.
#[test]
fn e1_events_already_in_cache_are_not_double_served() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let base_ts: u64 = 1_700_000_000;

    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([1u32]);
    kernel.timeline_authors.insert(author.clone());

    // Seed 2 events into the store.
    let ids = seed_events(&mut kernel, &keys, 2, base_ts);

    // Simulate a partial restart: clear the in-memory caches but leave
    // one event pre-populated in the cache (relay arrived first).
    let kept_id = ids[0].clone();
    let kept_event = StoredEvent {
        id: kept_id.clone(),
        author: author.clone(),
        kind: 1,
        created_at: base_ts,
        tags: vec![],
        content: "pre-cached from relay".to_string(),
        relay_count: 1,
    };
    kernel.events.clear();
    kernel.timeline.clear();
    kernel.metric_stored_events = 0;
    kernel.metric_note_events = 0;
    kernel.served_interest_shapes.clear();
    // Re-insert the "relay-delivered" event into the empty cache.
    kernel.events.insert(kept_id.clone(), kept_event);

    let cache_size_before = kernel.events.len();

    // cache-serve fires via sync_follow_feed_interests.
    kernel.sync_follow_feed_interests(&[author.clone()]);

    // The already-cached event must NOT cause the cache to grow by more than 1
    // (the second event that was in the store but not in the cache).
    let cache_size_after = kernel.events.len();
    assert!(
        cache_size_after <= cache_size_before + 1,
        "E1 dedup: events cache grew from {cache_size_before} to {cache_size_after}, \
         expected at most {}", cache_size_before + 1
    );
    assert!(
        kernel.events.contains_key(kept_id.as_str()),
        "E1 dedup: the pre-cached relay-delivered event must still be present"
    );
}

// ─── 4. Watermark ⇄ serve invariant ─────────────────────────────────────────

/// ADR-0045 §6 structural identity: `shape_to_store_queries` produces
/// `AuthorKind` for shapes with ≥1 author + ≥1 kind, and `KindTime` for
/// shapes with 0 authors + ≥1 kind.
///
/// This is a compile-time-enforced structural assertion — if `StoreQuery`
/// variants change, this test fails, forcing a deliberate alignment update.
#[test]
fn e1_watermark_serve_invariant_shapes_are_aligned() {
    use crate::planner::InterestShape;

    // Shape 1: 1 author + 1 kind → AuthorKind
    let mut shape_author = InterestShape::default();
    shape_author.authors = BTreeSet::from([hex_pk("ab")]);
    shape_author.kinds = BTreeSet::from([1u32]);
    let queries = shape_to_store_queries(&shape_author);
    assert_eq!(queries.len(), 1, "1 author + 1 kind must produce 1 AuthorKind query");
    match &queries[0] {
        StoreQuery::AuthorKind { kinds, .. } => {
            assert_eq!(kinds, &vec![1u32], "kinds must match");
        }
        other => panic!("expected AuthorKind, got {other:?}"),
    }

    // Shape 2: 2 authors + 1 kind → 2 AuthorKind queries (one per author)
    let mut shape_two_authors = InterestShape::default();
    shape_two_authors.authors = BTreeSet::from([hex_pk("aa"), hex_pk("bb")]);
    shape_two_authors.kinds = BTreeSet::from([1u32]);
    let queries2 = shape_to_store_queries(&shape_two_authors);
    assert_eq!(queries2.len(), 2, "2 authors + 1 kind must produce 2 AuthorKind queries");
    for q in &queries2 {
        assert!(
            matches!(q, StoreQuery::AuthorKind { .. }),
            "each per-author query must be AuthorKind"
        );
    }

    // Shape 3: 0 authors + 1 kind → KindTime
    let mut shape_kindtime = InterestShape::default();
    shape_kindtime.kinds = BTreeSet::from([30023u32]);
    let queries3 = shape_to_store_queries(&shape_kindtime);
    assert_eq!(queries3.len(), 1, "0 authors + 1 kind must produce 1 KindTime query");
    assert!(
        matches!(&queries3[0], StoreQuery::KindTime { .. }),
        "must produce KindTime for 0-author shape"
    );

    // Shape 4: 0 kinds → empty (wildcard; not covered by E1)
    let shape_no_kinds = InterestShape::default();
    let queries4 = shape_to_store_queries(&shape_no_kinds);
    assert!(queries4.is_empty(), "0 kinds must produce no queries (not covered by E1)");

    // Shape 5: has tags → empty (E2/E3 territory)
    let mut shape_tagged = InterestShape::default();
    shape_tagged.kinds = BTreeSet::from([1u32]);
    // tags: BTreeMap<TagKey, BTreeSet<String>>; insert an `#e` tag filter.
    shape_tagged.tags.insert(
        "e".to_string(),
        BTreeSet::from(["abc".to_string()]),
    );
    let queries5 = shape_to_store_queries(&shape_tagged);
    assert!(queries5.is_empty(), "tagged shapes must produce no queries (E2/E3)");
}

// ─── 5. Completion-key one-shot ───────────────────────────────────────────────

/// Once an interest has been served, `sync_follow_feed_interests` for the same
/// follow set must NOT re-serve the same events (the completion key gates it).
///
/// We verify this by checking that `kernel.events.len()` does not grow on a
/// second `sync_follow_feed_interests` call.
#[test]
fn e1_completion_key_prevents_re_serve() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let base_ts: u64 = 1_700_000_000;

    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([1u32]);
    kernel.timeline_authors.insert(author.clone());

    seed_events(&mut kernel, &keys, 3, base_ts);
    simulate_cold_restart(&mut kernel);

    // First serve.
    kernel.sync_follow_feed_interests(&[author.clone()]);
    let after_first = kernel.events.len();
    assert!(after_first > 0, "first sync must serve events");

    // Second sync — same follow set; completion key is already recorded.
    kernel.sync_follow_feed_interests(&[author.clone()]);
    let after_second = kernel.events.len();
    assert_eq!(
        after_second, after_first,
        "E1 one-shot: a second sync for the same follow set must not re-serve events"
    );
}

// ─── 6. Account-switch clears completion set ─────────────────────────────────

/// After `reconcile_follow_feed_after_identity_change`, the completion set is
/// cleared so the new account's interests get a fresh serve.
///
/// Strategy: run cache-serve for account A (populates completion set), record
/// the set is non-empty, then switch to account B. After the switch the
/// completion set must either be empty (cleared with no new serves) or contain
/// ONLY keys for B's interests (cleared + re-served for B). Either way, A's
/// old keys must NOT be present — verified by confirming the set was cleared at
/// least once before B's interests were registered.
///
/// We verify this indirectly: after the account switch, re-run
/// `sync_follow_feed_interests` for B's author and assert the served count
/// grows (it would be 0 if the completion key from A's serve was blocking).
#[test]
fn e1_account_switch_triggers_fresh_serve() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys_a = ::nostr::Keys::generate();
    let author_a = keys_a.public_key().to_hex();
    let keys_b = ::nostr::Keys::generate();
    let author_b = keys_b.public_key().to_hex();
    let base_ts: u64 = 1_700_000_000;

    // Account A: seed events, open follow-feed, run cache-serve.
    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([1u32]);
    kernel.timeline_authors.insert(author_a.clone());
    seed_events(&mut kernel, &keys_a, 2, base_ts);
    simulate_cold_restart(&mut kernel);
    kernel.sync_follow_feed_interests(&[author_a.clone()]);

    // The completion set must be non-empty after serving A's interests.
    assert!(
        !kernel.served_interest_shapes.is_empty(),
        "completion set must be non-empty after first serve (pre-condition)"
    );

    // Seed events for author B into the store (we add B to timeline_authors
    // temporarily so ingest_timeline_event will store them).
    kernel.timeline_authors.insert(author_b.clone());
    seed_events(&mut kernel, &keys_b, 2, base_ts + 100_000);
    // Remove B from timeline_authors again; B is not yet followed by A.
    kernel.timeline_authors.remove(&author_b);

    // Snapshot the completion set size before the switch.
    let keys_before_switch = kernel.served_interest_shapes.clone();

    // Switch to account B (sets active_account, registers B's follow-feed
    // interests, and calls clear_served_interest_shapes).
    kernel.events.clear();
    kernel.timeline.clear();
    kernel.metric_stored_events = 0;
    kernel.metric_note_events = 0;
    kernel.active_account = Some(author_b.clone());
    // Manually register B's follows in seed_contacts so reconcile has something
    // to work with. B follows author_a.
    kernel.seed_contacts.insert(author_b.clone(), vec![author_a.clone()]);
    kernel.reconcile_follow_feed_after_identity_change();

    // Verify that the clear + fresh serve actually ran. The evidence:
    //
    // 1. After the switch, events for author_a (B follows A) must appear in
    //    the `events` cache — they were served from the store for B's interests.
    //    If `clear_served_interest_shapes` was NOT called, the completion key
    //    would still be set from A's serve and the re-serve would be skipped,
    //    leaving `kernel.events` empty.
    //
    // 2. Since A's interests and B's interests (B follows A) share the same
    //    shape (author_a + kinds), the completion key happens to be the same.
    //    We can't assert the key is absent (B re-added it). We CAN assert that
    //    the serve DID fire by checking `kernel.events` is non-empty.
    assert!(
        !kernel.events.is_empty(),
        "E1 account-switch: events cache must be non-empty after switch — \
         cache-serve must have fired for B's interests (B follows A, A has stored events)"
    );
    let author_a_events: Vec<_> = kernel
        .events
        .values()
        .filter(|e| e.author == author_a)
        .collect();
    assert!(
        !author_a_events.is_empty(),
        "E1 account-switch: author_a's events must be served for account B (B follows A); \
         events in cache: {:?}",
        kernel.events.keys().collect::<Vec<_>>()
    );
}
