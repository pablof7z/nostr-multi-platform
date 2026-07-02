//! ADR-0045 §8 / issue #1086 — v1 exit criterion: the universal acceptance
//! test proving ALL four projection paths (feed, DM IngestParser, thread,
//! long-form) render from a warm store with ZERO relay connectivity after a
//! cold restart.

use super::universal_fixtures_support::{gift_wrap_json, open_interest, register_one, signed_event_json, CapturingIngestParser};
use crate::kernel::cache_serve_tests::{drain_cache_serves, hex_pk, open_author_interest, simulate_cold_restart};
use crate::kernel::Kernel;
use crate::planner::{InterestShape, NaddrCoord};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::SubKey;
use nmp_network::role::RelayRole;
use std::collections::BTreeSet;

/// Verbatim output from the failing case: each projection path asserts a
/// distinct error message so a regression is immediately attributable to the
/// specific engineering increment (E1 feed / E2 DM / E3 thread or long-form).
#[test]
fn universal_acceptance_all_four_projection_paths_from_store_no_relay() {
    // ── Identities ───────────────────────────────────────────────────────────
    let base_ts: u64 = 1_700_000_000;
    let receiver_keys = ::nostr::Keys::generate();
    let receiver_hex = receiver_keys.public_key().to_hex();
    let sender_keys = ::nostr::Keys::generate();
    let feed_author_keys = ::nostr::Keys::generate();
    let feed_author = feed_author_keys.public_key().to_hex();

    // ── 64-char hex target for #e thread (the "parent" event id) ─────────────
    // We fabricate a parent event id for the thread reply tag.
    let parent_id_hex = hex_pk("dead");

    // ── Long-form d_tag ───────────────────────────────────────────────────────
    let d_tag = "universal-test-article";

    // ── Phase 1: kernel with wired IngestParser for kind:1059 (E2) ───────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let dm_ingest_parser = CapturingIngestParser::new();
    kernel.register_ingest_parser(1059, dm_ingest_parser.clone());

    // Active account is the receiver (DM recipient and feed "self").
    kernel.active_account = Some(receiver_hex.clone());
    // Feed author is visible in the home timeline projection.
    kernel.timeline_authors.insert(feed_author.clone());

    // Pre-open the thread interest so `should_store_event` admits the thread
    // reply via `matches_active_open_interest`. The thread reply is kind:1 from
    // an author who is NOT in `timeline_authors`; without a matching open
    // interest the kernel's `ingest_timeline_event` admission gate would drop
    // it at Phase-1 ingest time and it would never reach the store.
    // Note: the open here also triggers a cache-serve (store is empty →
    // no-op scan) and marks the completion key as served. After
    // `simulate_cold_restart` the key is cleared and Phase 3 re-opens it.
    {
        let mut ts = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        ts.tags
            .insert("e".to_string(), BTreeSet::from([parent_id_hex.clone()]));
        open_interest(&mut kernel, 200, ts);
    }
    // The initial cache-serve for the empty store is a no-op — drain it away.
    drain_cache_serves(&mut kernel, 2);

    // ── Seed: 2 feed events (kind:1 from feed_author) ────────────────────────
    let feed_ev_1 = signed_event_json(&feed_author_keys, 1, "feed event alpha", vec![], base_ts);
    let feed_ev_2 = signed_event_json(&feed_author_keys, 1, "feed event beta", vec![], base_ts + 1);
    let feed_id_1 = feed_ev_1["id"].as_str().unwrap().to_string();
    let feed_id_2 = feed_ev_2["id"].as_str().unwrap().to_string();
    kernel.handle_event(RelayRole::Content, "wss://seed.relay/", "feed", &feed_ev_1);
    kernel.handle_event(RelayRole::Content, "wss://seed.relay/", "feed", &feed_ev_2);

    // ── Seed: 1 thread reply (kind:1 with #e tag to parent_id_hex) ───────────
    // The thread interest is pre-opened above so `matches_active_open_interest`
    // admits this kind:1 event from a non-followed author into the store.
    let thread_ev = signed_event_json(
        &sender_keys,
        1,
        "thread reply content",
        vec![vec![
            "e".to_string(),
            parent_id_hex.clone(),
            String::new(),
            "reply".to_string(),
        ]],
        base_ts + 2,
    );
    let thread_id = thread_ev["id"].as_str().unwrap().to_string();
    kernel.handle_event(
        RelayRole::Content,
        "wss://seed.relay/",
        "thread",
        &thread_ev,
    );

    // ── Seed: 1 long-form article (kind:30023 with #d tag) ───────────────────
    // kind:30023 goes through the wildcard arm in `handle_event` — stored
    // unconditionally (no `should_store_event` admission gate). No pre-open
    // needed.
    let longform_ev = signed_event_json(
        &sender_keys,
        30023,
        "# Universal Test Article\nProves E3 long-form cache-serve.",
        vec![
            vec!["d".to_string(), d_tag.to_string()],
            vec!["title".to_string(), "Universal Test Article".to_string()],
        ],
        base_ts + 3,
    );
    let longform_id = longform_ev["id"].as_str().unwrap().to_string();
    kernel.handle_event(
        RelayRole::Content,
        "wss://seed.relay/",
        "longform",
        &longform_ev,
    );

    // ── Seed: 1 DM gift-wrap (kind:1059, #p receiver_hex) ────────────────────
    // kind:1059 also goes through the wildcard arm — stored unconditionally.
    let (gift_wrap_json, gift_wrap_id) = gift_wrap_json(
        &sender_keys,
        &receiver_keys.public_key(),
        "universal test DM",
        base_ts + 4,
    );
    kernel.handle_event(
        RelayRole::Content,
        "wss://dm.relay/",
        "dm-inbox",
        &gift_wrap_json,
    );

    // Phase 1 postconditions:
    // The live chokepoint persists accepted events and notifies observers, but
    // feed rendering is owned by registered interests. Verify feed and
    // long-form rows are NOT in the read-cache yet so Phase 4's cache-serve
    // assertions are non-vacuous. The thread row is allowed to be cached here:
    // this test pre-opens a thread interest before seeding to exercise store
    // admission for non-followed authors.
    assert!(
        !kernel.events.contains_key(feed_id_1.as_str())
            && !kernel.events.contains_key(feed_id_2.as_str())
            && !kernel.events.contains_key(longform_id.as_str()),
        "Phase 1 pre-condition: store-seeded rows must not already be in events cache; \
         cache-serve will populate them in Phase 4",
    );
    // (raw observer tap removed from kernel in Step 2; dispatcher handles external sinks)
    assert!(
        !kernel.events.contains_key(longform_id.as_str()),
        "Phase 1 pre-condition: long-form must NOT be in events cache yet \
         (wildcard arm — cache-serve will populate it in Phase 4)"
    );

    // ── Phase 2: cold restart ─────────────────────────────────────────────────
    // Clear in-memory caches (store persists — same in-process Arc<dyn EventStore>).
    // Reset seen lists so Phase 4 assertions reflect only cache-serve delivery.
    simulate_cold_restart(&mut kernel);
    dm_ingest_parser.clear();

    assert!(
        kernel.events.is_empty(),
        "Phase 2: events cache must be empty after restart"
    );
    assert!(
        kernel.timeline.is_empty(),
        "Phase 2: timeline must be empty after restart"
    );
    assert!(
        dm_ingest_parser.seen().is_empty(),
        "Phase 2: IngestParser seen list must be cleared before serve"
    );

    // ── Phase 3: open interests and drain cache-serves (ZERO relay) ───────────
    // Fresh keys force `changed=true` after the Phase-1 pre-open. The feed uses
    // the reduced author-set shape through the generic interest path.
    open_author_interest(
        &mut kernel,
        "universal-feed-phase3",
        [feed_author.clone()],
        [1u32],
    );

    // E3 — thread: register Etag interest with a fresh key → newly_installed=true.
    {
        let mut thread_shape = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        thread_shape
            .tags
            .insert("e".to_string(), BTreeSet::from([parent_id_hex.clone()]));
        let thread_key = SubKey::new(("thread-phase3", &parent_id_hex));
        register_one(
            &mut kernel,
            "test-thread-phase3",
            thread_key,
            thread_shape,
            "test-phase3-thread",
        );
    }

    // E3 — long-form: register KindDtag interest with a fresh key.
    {
        let author_for_longform = sender_keys.public_key().to_hex();
        let mut longform_shape = InterestShape {
            kinds: BTreeSet::from([30023u32]),
            ..Default::default()
        };
        longform_shape.addresses.insert(NaddrCoord {
            pubkey: author_for_longform.clone(),
            kind: 30023,
            d_tag: d_tag.to_string(),
        });
        let lf_key = SubKey::new(("longform-phase3", &author_for_longform, d_tag));
        register_one(
            &mut kernel,
            "test-longform-phase3",
            lf_key,
            longform_shape,
            "test-phase3-longform",
        );
    }

    // E2 — DM inbox: register Ptag interest with a fresh key.
    {
        let mut dm_shape = InterestShape {
            kinds: BTreeSet::from([1059u32]),
            ..Default::default()
        };
        dm_shape
            .tags
            .insert("p".to_string(), BTreeSet::from([receiver_hex.clone()]));
        let dm_key = SubKey::new(("dm-phase3", &receiver_hex));
        register_one(
            &mut kernel,
            "test-dm-phase3",
            dm_key,
            dm_shape,
            "test-phase3-dm",
        );
    }

    // Drain: the feed interest open ran one synchronous step; continue
    // until the queue is empty. These small fixtures finish in ≤ 2 ticks.
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: assert ALL four projection paths rendered from store ─────────

    // E1 — feed events in read-cache AND timeline.
    assert!(
        kernel.events.contains_key(feed_id_1.as_str()),
        "E1 FAIL: feed_ev_1 ({feed_id_1}) must be in events cache after cold-restart serve"
    );
    assert!(
        kernel.events.contains_key(feed_id_2.as_str()),
        "E1 FAIL: feed_ev_2 ({feed_id_2}) must be in events cache after cold-restart serve"
    );
    assert!(
        kernel
            .timeline
            .iter()
            .any(|id| id == &feed_id_1 || id == &feed_id_2),
        "E1 FAIL: at least one feed event must appear in the timeline after cache-serve \
         (timeline len={})",
        kernel.timeline.len()
    );

    // E3 — thread reply in read-cache.
    assert!(
        kernel.events.contains_key(thread_id.as_str()),
        "E3 FAIL: thread reply ({thread_id}) must be in events cache after cold-restart \
         Etag cache-serve"
    );

    // E3 — long-form article in read-cache.
    assert!(
        kernel.events.contains_key(longform_id.as_str()),
        "E3 FAIL: long-form article ({longform_id}) must be in events cache after cold-restart \
         KindDtag cache-serve"
    );

    // E2 — DM gift-wrap reached the IngestParser seam (proving the decrypt seam
    // fires from store just as it does from live relay delivery). After raw-tap
    // PR-2, cache-serve emits ONLY via `ingest_dispatcher.dispatch()` — no
    // raw-observer fan-out from the store path.
    let dm_ingest_seen = dm_ingest_parser.seen();
    assert!(
        dm_ingest_seen.contains(&1059),
        "E2 FAIL: IngestParser must receive kind:1059 after cold-restart Ptag cache-serve; \
         got {dm_ingest_seen:?} — the DmInboxProjection / MarmotIngestParser decrypt seam \
         would not fire after restart"
    );
    let gift_wrap_in_cache = kernel.events.contains_key(gift_wrap_id.as_str());
    assert!(
        gift_wrap_in_cache,
        "E2 FAIL: gift-wrap ({gift_wrap_id}) must be in events cache after cold-restart serve"
    );
}
