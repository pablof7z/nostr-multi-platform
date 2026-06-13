//! ADR-0045 E2+E3 — **Universal acceptance test** (closes v1-blocker #1086).
//!
//! Requirement (owner-decided, issue #1086): populate a store with feed events
//! + a DM gift-wrap + a thread reply + a long-form article; fresh kernel, zero
//! relay connectivity; open the standard interests; assert feed, DM (IngestParser),
//! thread, and long-form projections ALL render from the store.
//!
//! This is the v1 exit criterion for ADR-0045 §8: one test that falsifies the
//! complete seam. If any engineering increment (E1 feed, E2 DM, E3 thread /
//! long-form) is broken, this test fails.
//!
//! ## Structure
//!
//! - **Phase 1 (seed)**: events are ingested through the live ingest path
//!   (`handle_event` — Schnorr-verify + store + observer fan-out) to populate
//!   the persistent store exactly as production does.
//!
//! - **Phase 2 (cold restart)**: the in-memory caches (`events`, `timeline`)
//!   are cleared — simulating a process restart that discards all in-memory state.
//!
//! - **Phase 3 (serve)**: cache-serve interests are enqueued for each shape
//!   and drained under the aggregate budget. Each interest is opened without
//!   any relay connection.
//!
//! - **Phase 4 (assert)**: every projection path is asserted non-empty.
//!
//! ## Why IngestParser, not DmInboxProjection directly
//!
//! `DmInboxProjection` lives in `nmp-nip17`, which depends on `nmp-core`,
//! creating a circular compile dependency if we imported it here. This test
//! instead verifies the seam that `DmInboxProjection` and `MarmotIngestParser`
//! both ride:
//!
//! - **IngestParser** (`ingest_dispatcher.dispatch()`): all former raw-tap
//!   consumers (NIP-17 DM inbox since PR-1, Marmot since PR-2) now ride this
//!   seam exclusively. `CapturingIngestParser` stands in here to avoid the
//!   circular dep on `nmp-nip17`.
//!
//! The decrypt path itself is exercised by
//! `nmp-nip17::inbox::tests::received_dm_surfaces_in_the_conversation`.

use super::cache_serve_tests::{drain_cache_serves, hex_pk, simulate_cold_restart};
use super::*;
use crate::actor::{new_raw_event_observer_slot, register_rust_raw_observer, KindFilter, RawEventObserver};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, NaddrCoord,
};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::VerifiedEvent;
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use crate::substrate::IngestParser;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

// ─── Fixtures ────────────────────────────────────────────────────────────────

/// An `IngestParser` that records the kind of every event it receives.
/// Used to verify kind:1059 events reach the IngestParser seam after cache-serve
/// (PR-1 of the raw-tap retirement ladder).
struct CapturingIngestParser {
    seen_kinds: Mutex<Vec<u32>>,
}

impl CapturingIngestParser {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen_kinds: Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<u32> {
        self.seen_kinds.lock().unwrap().clone()
    }

    fn clear(&self) {
        self.seen_kinds.lock().unwrap().clear();
    }
}

impl IngestParser for CapturingIngestParser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.seen_kinds.lock().unwrap().push(evt.raw().kind);
    }
}

/// A raw observer that records the kind of every event it receives.
/// Used to verify kind:1059 events reach the raw tap after cache-serve.
struct CapturingRawObserver {
    seen_kinds: Mutex<Vec<u32>>,
}

impl CapturingRawObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen_kinds: Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<u32> {
        self.seen_kinds.lock().unwrap().clone()
    }

    fn clear(&self) {
        self.seen_kinds.lock().unwrap().clear();
    }
}

impl RawEventObserver for CapturingRawObserver {
    fn on_raw_event(&self, kind: u32, _json: &str) {
        self.seen_kinds.lock().unwrap().push(kind);
    }

    fn on_raw_event_with_source(&self, kind: u32, _json: &str, _source: Option<&str>) {
        self.seen_kinds.lock().unwrap().push(kind);
    }
}

/// Build a NIP-01 JSON `Value` for a signed event via `handle_event`-compatible
/// format (same pattern as `raw_event_observer_tests::signed_event_value`).
fn signed_event_json(
    keys: &::nostr::Keys,
    kind: u32,
    content: &str,
    tags: Vec<Vec<String>>,
    created_at: u64,
) -> serde_json::Value {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let nostr_tags: Vec<Tag> = tags
        .iter()
        .map(|t| Tag::parse(t.as_slice()).expect("well-formed tag"))
        .collect();
    let ev = EventBuilder::new(Kind::from(kind as u16), content)
        .tags(nostr_tags)
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    let tag_vecs: Vec<Vec<String>> = ev
        .tags
        .iter()
        .map(|t: &::nostr::Tag| t.as_slice().to_vec())
        .collect();
    serde_json::json!({
        "id": ev.id.to_hex(),
        "pubkey": ev.pubkey.to_hex(),
        "created_at": ev.created_at.as_secs(),
        "kind": ev.kind.as_u16(),
        "tags": tag_vecs,
        "content": ev.content.clone(),
        "sig": ev.sig.to_string(),
    })
}

/// Build and return a kind:1059 gift-wrap JSON `Value` from `sender` to
/// `receiver`, using `nmp_nip59::gift_wrap_local` — the same pure seal/wrap
/// composition the production DM chain assembles on the actor thread.
fn gift_wrap_json(
    sender: &::nostr::Keys,
    receiver: &::nostr::PublicKey,
    content: &str,
    created_at: u64,
) -> (serde_json::Value, String) {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};

    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(vec![Tag::public_key(*receiver)])
        .custom_created_at(Timestamp::from(created_at))
        .build(sender.public_key());

    let envelope =
        nmp_nip59::gift_wrap_local(sender, receiver, &rumor, Timestamp::from(created_at))
            .expect("gift_wrap_local succeeds with local keys");

    let tag_vecs: Vec<Vec<String>> = envelope
        .tags
        .iter()
        .map(|t: &::nostr::Tag| t.as_slice().to_vec())
        .collect();
    let json = serde_json::json!({
        "id": envelope.id.to_hex(),
        "pubkey": envelope.pubkey.to_hex(),
        "created_at": envelope.created_at.as_secs(),
        "kind": envelope.kind.as_u16(),
        "tags": tag_vecs,
        "content": envelope.content.clone(),
        "sig": envelope.sig.to_string(),
    });
    let id = envelope.id.to_hex();
    (json, id)
}

/// Construct a `SubIdentity` for opening a generic non-feed interest.
fn sub_identity(seed: u64) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(seed),
        SubKey::new(seed),
        SubScope::Global,
    )
}

/// Open a cache-serve interest for `shape` and return whether it was newly
/// installed. Mirrors the production `open_interest_sub` path.
fn open_interest(kernel: &mut Kernel, seed: u64, shape: InterestShape) -> bool {
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    kernel.open_interest_sub(sub_identity(seed), interest)
}

// ─── The universal acceptance test ───────────────────────────────────────────

/// ADR-0045 §8 / issue #1086 — v1 exit criterion.
///
/// Proves that ALL four projection paths (feed, DM IngestParser, thread,
/// long-form) render from a warm store with ZERO relay connectivity after a
/// cold restart.
///
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
    // A raw observer is also wired so the Phase-1 sanity check (live ingest
    // fires raw observers) can still run — the raw-observer path is NOT touched
    // by this PR (it remains for chirp-tui / hl mirror on live relay delivery).
    // The cache-serve E2 assertion uses the IngestParser seam exclusively
    // (raw-tap PR-2 removed the dual fan-out from cache-serve).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let dm_ingest_parser = CapturingIngestParser::new();
    kernel.register_ingest_parser(1059, dm_ingest_parser.clone());
    let slot = new_raw_event_observer_slot();
    let observer = CapturingRawObserver::new();
    // kind:1059 only — confirms the live ingest raw tap still fires during seeding.
    register_rust_raw_observer(&slot, KindFilter::from_kinds([1059u32]), observer.clone());
    kernel.set_raw_event_observers_handle(slot);

    // Active account is the receiver (DM recipient and feed "self").
    kernel.active_account = Some(receiver_hex.clone());
    // Feed author is followed.
    kernel.follow_feed_kinds = BTreeSet::from([1u32]);
    kernel.timeline_authors.insert(feed_author.clone());

    // Pre-open the thread interest so `should_store_event` admits the thread
    // reply via `matches_active_open_interest`. The thread reply is kind:1 from
    // an author who is NOT in `timeline_authors`; without a matching open
    // interest the kernel's `ingest_timeline_event` admission gate would drop
    // it at Phase-1 ingest time and it would never reach the store.
    // Note: the open here also calls `enqueue_cache_serve` (store is empty →
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
        vec![vec!["e".to_string(), parent_id_hex.clone(), String::new(), "reply".to_string()]],
        base_ts + 2,
    );
    let thread_id = thread_ev["id"].as_str().unwrap().to_string();
    kernel.handle_event(RelayRole::Content, "wss://seed.relay/", "thread", &thread_ev);

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
    kernel.handle_event(RelayRole::Content, "wss://seed.relay/", "longform", &longform_ev);

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
    // - Feed events (kind:1 from timeline author) land in the in-memory cache.
    // - Thread reply (kind:1 from non-followed author, but open interest matches)
    //   is stored AND admitted into kernel.events (matches_active_open_interest).
    // - Long-form / gift-wrap go through wildcard arm → stored, but NOT in
    //   kernel.events (wildcard arm does NOT call `self.events.insert`).
    assert!(
        kernel.events.contains_key(feed_id_1.as_str()),
        "Phase 1: feed_ev_1 must be in events cache after ingest (timeline author)"
    );
    assert!(
        kernel.events.contains_key(feed_id_2.as_str()),
        "Phase 1: feed_ev_2 must be in events cache after ingest (timeline author)"
    );
    assert!(
        kernel.events.contains_key(thread_id.as_str()),
        "Phase 1: thread reply must be in events cache (admitted via open interest)"
    );
    // Raw observer received kind:1059 during seeding — sanity check that the
    // live ingest raw tap still fires (chirp-tui / hl mirror path, NOT touched
    // by this PR). The E2 cache-serve assertion uses the IngestParser seam.
    let seen_on_seed = observer.seen();
    assert!(
        seen_on_seed.contains(&1059),
        "Phase 1: raw observer must see kind:1059 on live seed ingest; got {seen_on_seed:?}"
    );
    // Long-form (kind:30023) goes through wildcard arm → store only (not events cache).
    // Verify it is NOT in the cache yet to make the Phase 4 assertion non-vacuous.
    assert!(
        !kernel.events.contains_key(longform_id.as_str()),
        "Phase 1 pre-condition: long-form must NOT be in events cache yet \
         (wildcard arm — cache-serve will populate it in Phase 4)"
    );

    // ── Phase 2: cold restart ─────────────────────────────────────────────────
    // Clear in-memory caches (store persists — same in-process Arc<dyn EventStore>).
    // Reset seen lists so Phase 4 assertions reflect only cache-serve delivery.
    simulate_cold_restart(&mut kernel);
    observer.clear();
    dm_ingest_parser.clear();

    assert!(kernel.events.is_empty(), "Phase 2: events cache must be empty after restart");
    assert!(kernel.timeline.is_empty(), "Phase 2: timeline must be empty after restart");
    assert!(observer.seen().is_empty(), "Phase 2: observer must be cleared before serve");
    assert!(
        dm_ingest_parser.seen().is_empty(),
        "Phase 2: IngestParser seen list must be cleared before serve"
    );

    // ── Phase 3: open interests and drain cache-serves (ZERO relay) ───────────
    //
    // `open_interest_sub` calls `enqueue_cache_serve` ONLY when the interest is
    // NEWLY installed (lifecycle registry check). The thread interest was
    // pre-opened in Phase 1 (to admit the event at ingest time), so the registry
    // still holds the slot. Calling `open_interest_sub` again with the same
    // identity would find the slot and return `newly_installed=false`.
    //
    // Solution: call `enqueue_cache_serve` directly with a stable key derived
    // from a distinct sub_key that is NOT in the pre-open's registry slot.
    // `simulate_cold_restart` cleared `served_interest_shapes` so the completion
    // key is fresh; `enqueue_cache_serve` idempotently skips duplicates via the
    // pending queue check.
    //
    // For the feed we use `sync_follow_feed_interests` (the production entry
    // point), which computes its own SubKey from the active account + authors
    // and is not affected by the registry collision.

    // E1 — feed: sync_follow_feed_interests enqueues per-author AuthorKind serves.
    kernel.sync_follow_feed_interests(&[feed_author.clone()]);

    // E3 — thread: directly enqueue Etag cache-serve (bypasses registry check).
    {
        let mut thread_shape = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        thread_shape
            .tags
            .insert("e".to_string(), BTreeSet::from([parent_id_hex.clone()]));
        // Use a fresh SubKey that doesn't collide with the pre-open registration.
        let thread_key = crate::subs::SubKey::new(("thread-phase3", &parent_id_hex));
        let completion_key =
            crate::kernel::cache_serve::completion_key_for_interest(&thread_key, &thread_shape);
        kernel.enqueue_cache_serve(&thread_shape, completion_key);
    }

    // E3 — long-form: directly enqueue KindDtag cache-serve.
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
        let lf_key = crate::subs::SubKey::new(("longform-phase3", &author_for_longform, d_tag));
        let completion_key =
            crate::kernel::cache_serve::completion_key_for_interest(&lf_key, &longform_shape);
        kernel.enqueue_cache_serve(&longform_shape, completion_key);
    }

    // E2 — DM inbox: directly enqueue Ptag cache-serve (raw observer dispatch).
    {
        let mut dm_shape = InterestShape {
            kinds: BTreeSet::from([1059u32]),
            ..Default::default()
        };
        dm_shape
            .tags
            .insert("p".to_string(), BTreeSet::from([receiver_hex.clone()]));
        let dm_key = crate::subs::SubKey::new(("dm-phase3", &receiver_hex));
        let completion_key =
            crate::kernel::cache_serve::completion_key_for_interest(&dm_key, &dm_shape);
        kernel.enqueue_cache_serve(&dm_shape, completion_key);
    }

    // Drain: `sync_follow_feed_interests` ran one synchronous step; continue
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
        kernel.timeline.iter().any(|id| id == &feed_id_1 || id == &feed_id_2),
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

/// PR-2 rawtap retirement — cache-serve feeds kind:1059 exclusively via IngestParser.
///
/// Verifies that `feed_served_event` dispatches kind:1059 events ONLY through the
/// `EventIngestDispatcher` seam after PR-2 removes the transitional dual fan-out.
/// No raw-observer delivery from cache-serve — all former raw-tap consumers
/// (NIP-17 DM inbox since PR-1, Marmot since PR-2) now ride `IngestParser`.
/// Uses a `CapturingIngestParser` to avoid the circular dep on `nmp-nip17`.
#[test]
fn e2_cache_serve_feeds_ingest_parser_for_kind_1059() {
    let base_ts: u64 = 1_700_000_000;
    let receiver_keys = ::nostr::Keys::generate();
    let receiver_hex = receiver_keys.public_key().to_hex();
    let sender_keys = ::nostr::Keys::generate();

    // ── Kernel with wired IngestParser for kind:1059 ──────────────────────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Wire the capturing IngestParser into the kernel's shared dispatcher slot.
    let ingest_parser = CapturingIngestParser::new();
    kernel.register_ingest_parser(1059, ingest_parser.clone());

    kernel.active_account = Some(receiver_hex.clone());

    // ── Phase 1: seed a kind:1059 gift-wrap into the store ────────────────────
    let (gift_wrap_json_val, gift_wrap_id) = gift_wrap_json(
        &sender_keys,
        &receiver_keys.public_key(),
        "parser cache-serve test",
        base_ts,
    );
    kernel.handle_event(
        RelayRole::Content,
        "wss://relay.test/",
        "dm",
        &gift_wrap_json_val,
    );

    // Confirm the parser received it on ingest (live path).
    assert!(
        ingest_parser.seen().contains(&1059),
        "Phase 1: IngestParser must see kind:1059 on live ingest"
    );

    // ── Phase 2: cold restart — clear caches + reset counters ─────────────────
    simulate_cold_restart(&mut kernel);
    ingest_parser.clear();

    assert!(
        ingest_parser.seen().is_empty(),
        "Phase 2: IngestParser seen list must be cleared before cache-serve"
    );

    // ── Phase 3: enqueue and drain cache-serve for kind:1059 ──────────────────
    {
        let mut dm_shape = InterestShape {
            kinds: BTreeSet::from([1059u32]),
            ..Default::default()
        };
        dm_shape
            .tags
            .insert("p".to_string(), BTreeSet::from([receiver_hex.clone()]));
        let dm_key = crate::subs::SubKey::new(("ingest-parser-test", &receiver_hex));
        let completion_key =
            crate::kernel::cache_serve::completion_key_for_interest(&dm_key, &dm_shape);
        kernel.enqueue_cache_serve(&dm_shape, completion_key);
    }
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: IngestParser must receive kind:1059 from cache-serve ─────────
    // No raw-observer assertion: raw-tap PR-2 removed the dual fan-out from
    // cache-serve. Cache-serve now delivers exclusively via IngestParser.
    let ingest_seen = ingest_parser.seen();
    assert!(
        ingest_seen.contains(&1059),
        "E2/PR-2 FAIL: IngestParser must receive kind:1059 after cold-restart cache-serve; \
         got {ingest_seen:?} — the DmInboxProjection / MarmotIngestParser would not decrypt \
         after restart"
    );

    assert!(
        kernel.events.contains_key(gift_wrap_id.as_str()),
        "E2/PR-2 FAIL: gift-wrap must be in events cache after cache-serve"
    );
}
