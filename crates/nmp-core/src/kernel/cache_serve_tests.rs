//! ADR-0045 E1 — store-cache serve acceptance tests.
//!
//! These tests verify the ADR-0045 E1 invariants:
//!
//! 1. **Universal acceptance** — on second launch (in-memory caches cleared,
//!    store warm), `sync_follow_feed_interests` drives cache-serve and
//!    re-populates `events` and `timeline` from the store without any relay
//!    connectivity.
//!
//! 2. **Serve depth = 1× visible window** — a store holding more events than
//!    the consumer's visible window serves at most `visible_limit` events for
//!    one interest (owner decision 2026-06-12; `shape.limit` is the wire cap,
//!    not the render window).
//!
//! 3. **Aggregate per-tick budget + chunked continuation** (ADR §5) and the
//!    **watermark ⇄ serve invariant** (§6) live in
//!    `cache_serve_budget_tests.rs` (500-LOC file ceiling); the shared
//!    fixtures below are `pub(super)` for that sibling.
//!
//! 4. **Dedup-on-redelivery** — events already in the `events` cache (from a
//!    prior relay deliver) are skipped on cache-serve; the cache is not
//!    double-populated.
//!
//! 6. **Completion-key one-shot** — re-syncing the same follow set does not
//!    re-serve (the completion key gates it).
//!
//! 7. **Account-switch clears completion set + queue** — after
//!    `reconcile_follow_feed_after_identity_change`, the new account's
//!    interests get a fresh serve.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::collections::BTreeSet;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Build a 64-char lowercase hex string by repeating `prefix` until 64 chars.
pub(super) fn hex_pk(prefix: &str) -> String {
    let padded: String = prefix
        .chars()
        .chain(std::iter::repeat('0'))
        .take(64)
        .collect();
    padded
}

/// Signed kind:1 event helper — mirrors `ingest_tests::signed_note`.
pub(super) fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
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
pub(super) fn seed_events(
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

/// Register a `LogicalInterest` (from `shape`) into the live interest registry
/// under `sub_key`, WITHOUT enqueuing or draining a cache-serve.
///
/// #1380: the truncated-serve read view (`recompute_truncated_query_keys`)
/// resolves completion-key marks back to query keys through the registry, so a
/// truncated cursor-less interest must be present in the registry under the SAME
/// `SubKey` its serve's `completion_key` is derived from. Production install
/// paths register the interest before serving; tests that drive
/// `enqueue_cache_serve` directly (to control budget orchestration) use this to
/// install the matching registry slot without the synchronous drain
/// `open_interest_sub` would perform.
pub(super) fn register_interest_for_test(
    kernel: &mut Kernel,
    sub_key: crate::subs::SubKey,
    shape: &crate::planner::InterestShape,
) {
    use crate::planner::{InterestLifecycle, InterestScope, LogicalInterest};
    use crate::subs::sub_key::{SubIdentity, SubOwnerKey, SubScope};

    let interest = LogicalInterest {
        scope: InterestScope::Global,
        shape: shape.clone(),
        lifecycle: InterestLifecycle::Tailing,
        ..LogicalInterest::default()
    };
    let identity = SubIdentity::new(
        SubOwnerKey::new("cache-serve-test-owner"),
        sub_key,
        SubScope::Global,
    );
    kernel
        .lifecycle_mut()
        .registry_mut()
        .set_sub(identity, interest);
}

/// Clear `kernel.events` and `kernel.timeline` to simulate a cold second
/// launch (store persisted, in-memory caches empty).
pub(super) fn simulate_cold_restart(kernel: &mut Kernel) {
    kernel.events.clear();
    kernel.timeline.clear();
    kernel.metric_stored_events = 0;
    kernel.metric_note_events = 0;
    // Clear the served-interest completion set + pending queue so the next
    // `sync_follow_feed_interests` triggers a fresh cache-serve.
    kernel.clear_served_interest_shapes();
}

/// Drain the cache-serve continuation queue, asserting each step respects the
/// aggregate tick budget (served-per-step can never exceed visits-per-step,
/// which is capped at the budget). Returns the number of steps taken.
pub(super) fn drain_cache_serves(kernel: &mut Kernel, max_steps: usize) -> usize {
    let tick_budget = kernel.visible_limit * 2;
    let mut steps = 0usize;
    while kernel.has_pending_cache_serves() {
        let before = kernel.events.len();
        kernel.run_cache_serve_step();
        let served = kernel.events.len() - before;
        assert!(
            served <= tick_budget,
            "aggregate budget: one step served {served} events, budget is {tick_budget}"
        );
        steps += 1;
        assert!(
            steps <= max_steps,
            "cache-serve continuation did not finish within {max_steps} steps"
        );
    }
    steps
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
/// This is the central falsifiability probe: if the serve seam is broken or
/// not enqueued, `kernel.events` stays empty and the test fails.
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

    // Phase 3: re-open the follow-feed interest (enqueues serves + drains one
    // aggregate-budget chunk; 3 events fit in one chunk).
    kernel.sync_follow_feed_interests(&[author.clone()]);
    drain_cache_serves(&mut kernel, 4);

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

// ─── 2. Serve depth = 1× visible window ─────────────────────────────────────

/// ADR §4 (owner decision 2026-06-12): the serve depth for one interest is 1×
/// the consumer's visible window — NOT `shape.limit` (the follow feed's wire
/// cap is `Some(1000)`) and NOT a fixed constant.
///
/// Seeds `visible_limit + 5` events from one author, cold-restarts, serves,
/// drains, and asserts exactly `visible_limit` events were served — the
/// 5 events past the window stay on disk (the snapshot cannot show them).
#[test]
fn e1_serve_depth_is_the_consumers_visible_window() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let base_ts: u64 = 1_700_000_000;

    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([1u32]);
    kernel.timeline_authors.insert(author.clone());

    let over_window = DEFAULT_VISIBLE_LIMIT + 5;
    seed_events(&mut kernel, &keys, over_window, base_ts);
    assert_eq!(kernel.events.len(), over_window);

    simulate_cold_restart(&mut kernel);

    kernel.sync_follow_feed_interests(&[author.clone()]);
    drain_cache_serves(&mut kernel, 8);

    assert_eq!(
        kernel.events.len(),
        DEFAULT_VISIBLE_LIMIT,
        "serve depth must be exactly the visible window ({DEFAULT_VISIBLE_LIMIT}), \
         not shape.limit (1000) and not the full store ({over_window})"
    );
    // And the window must hold the NEWEST events (newest-first index scan):
    // the oldest 5 seeded events are the ones left on disk.
    let oldest_served = kernel
        .events
        .values()
        .map(|e| e.created_at)
        .min()
        .expect("events non-empty");
    assert_eq!(
        oldest_served,
        base_ts + 5,
        "the 5 events past the window must be the OLDEST ones (newest-first serve)"
    );
}

// ─── 4. Dedup-on-redelivery ───────────────────────────────────────────────────

/// Events already in `kernel.events` (from a live relay deliver that ran
/// BEFORE the interest was also served from the store) must NOT be
/// double-inserted.
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
    simulate_cold_restart(&mut kernel);
    // Re-insert the "relay-delivered" event into the empty cache.
    kernel.events.insert(kept_id.clone(), kept_event);

    let cache_size_before = kernel.events.len();

    kernel.sync_follow_feed_interests(&[author.clone()]);
    drain_cache_serves(&mut kernel, 4);

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
    // The relay-delivered copy keeps its provenance: relay_count stays 1
    // (cache-serve never overwrites a live-delivered cache entry; served
    // entries carry relay_count 0 — the de-facto LocalStore marker).
    assert_eq!(
        kernel.events[kept_id.as_str()].relay_count,
        1,
        "cache-serve must not clobber the relay-delivered entry's relay_count"
    );
}

// ─── 6. Completion-key one-shot ───────────────────────────────────────────────

/// Once an interest has been served, `sync_follow_feed_interests` for the same
/// follow set must NOT re-serve the same events (the completion key gates it).
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
    drain_cache_serves(&mut kernel, 4);
    let after_first = kernel.events.len();
    assert!(after_first > 0, "first sync must serve events");

    // Second sync — same follow set; completion keys already recorded, so
    // nothing is enqueued and nothing is served.
    kernel.sync_follow_feed_interests(&[author.clone()]);
    drain_cache_serves(&mut kernel, 4);
    assert_eq!(
        kernel.events.len(),
        after_first,
        "E1 one-shot: a second sync for the same follow set must not re-serve events"
    );
}

// ─── 7. Account-switch clears completion set + queue ─────────────────────────

/// After `reconcile_follow_feed_after_identity_change`, the completion set
/// (and pending queue) is cleared so the new account's interests get a fresh
/// serve.
///
/// Evidence: after the switch, events for author_a (whom B follows) appear in
/// the `events` cache — served from the store for B's interests. If
/// `clear_served_interest_shapes` were not called, the completion key from
/// A's serve (same shape: author_a + kinds) would gate the re-serve and the
/// cache would stay empty.
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
    drain_cache_serves(&mut kernel, 4);

    // The completion set must be non-empty after serving A's interests.
    assert!(
        !kernel.served_interest_shapes.is_empty(),
        "completion set must be non-empty after first serve (pre-condition)"
    );

    // Switch to account B (B follows author_a).
    kernel.events.clear();
    kernel.timeline.clear();
    kernel.metric_stored_events = 0;
    kernel.metric_note_events = 0;
    kernel.active_account = Some(author_b.clone());
    kernel.seed_contacts.insert(author_b.clone(), vec![author_a.clone()]);
    kernel.reconcile_follow_feed_after_identity_change();
    drain_cache_serves(&mut kernel, 4);

    // The fresh serve must have fired for B's interests: author_a's stored
    // events reappear. (A's serve used the SAME shape/completion key, so this
    // can only pass if the switch cleared the completion set.)
    let author_a_events = kernel
        .events
        .values()
        .filter(|e| e.author == author_a)
        .count();
    assert!(
        author_a_events > 0,
        "E1 account-switch: author_a's events must be re-served for account B \
         (B follows A); an uncleared completion set would gate the serve. \
         events in cache: {:?}",
        kernel.events.keys().collect::<Vec<_>>()
    );
}
