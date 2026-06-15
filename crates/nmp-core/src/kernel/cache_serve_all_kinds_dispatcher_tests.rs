//! Cache-serve regression test: an all-kinds range parser receives kind:1
//! events served from the store.
//!
//! Gap closed by this PR (Finding 3): `shape_needs_ingest_parser_dispatch`
//! only returned true for `#p`+kind:1059 DM shapes, so a KindTime / AuthorKind
//! cache-serve for kind:1 (the follow-feed) would never call
//! `ingest_dispatcher_slot()…dispatch()` for a registered all-kinds range parser
//! (e.g. chirp-tui's `RawCacheIngestParser`, slot `"chirp-tui.raw-cache"`).
//!
//! The architecturally-right fix (owner doctrine: one uniform mechanism):
//! replace the hardcoded shape allowlist with a per-kind registry query
//! (`EventIngestDispatcher::is_interested(kind)`) so ANY registered parser
//! — including future ones — causes cache-serve dispatch without code changes.

use super::cache_serve_tests::{
    drain_cache_serves, hex_pk, seed_events, signed_note, simulate_cold_restart,
};
use super::*;
use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::VerifiedEvent;
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use crate::substrate::IngestParser;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

// ─── Fixtures ────────────────────────────────────────────────────────────────

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
}

impl IngestParser for CapturingIngestParser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.seen_kinds.lock().unwrap().push(evt.raw().kind);
    }
}

fn sub_id(seed: u64) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(seed),
        SubKey::new(seed),
        SubScope::Global,
    )
}

fn open_author_interest(kernel: &mut Kernel, seed: u64, author_hex: &str) {
    let shape = InterestShape {
        authors: BTreeSet::from([author_hex.to_string()]),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    kernel.open_interest_sub(sub_id(seed), interest);
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// PRIMARY CONTRACT: a cache-served kind:1 event reaches an all-kinds range
/// parser registered on the `EventIngestDispatcher`.
///
/// This guards the chirp-tui `RawCacheIngestParser` (`0..u32::MAX`) use case:
/// the "View raw event" modal must see feed notes that are served from the store
/// on cold restart (not just live-relay notes).
#[test]
fn cache_served_kind1_reaches_all_kinds_ingest_parser() {
    let base_ts: u64 = 1_700_000_100;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // ── Phase 1: seed events into the store via live ingest ───────────────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.timeline_authors.insert(author.clone());

    // Wire all-kinds parser BEFORE seeding — it should also fire on live ingest.
    let parser = CapturingIngestParser::new();
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.replace_range_parser(
            0..u32::MAX,
            "test.all-kinds",
            Arc::clone(&parser) as Arc<dyn IngestParser>,
        );
    }

    let _ids = seed_events(&mut kernel, &keys, 3, base_ts);
    // Live ingest fires (timeline path + our fix).
    let seen_live = parser.seen();
    assert_eq!(
        seen_live.len(),
        3,
        "all-kinds parser must fire for each kind:1 on live ingest; got {seen_live:?}",
    );

    // ── Phase 2: cold restart (in-memory caches gone, store intact) ───────────
    simulate_cold_restart(&mut kernel);
    // Clear parser state so Phase 3 assertion is unambiguous.
    *parser.seen_kinds.lock().unwrap() = Vec::new();

    // ── Phase 3: open interest and drain cache-serve ───────────────────────────
    open_author_interest(&mut kernel, 10, &author);
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: assert cache-served events reached the all-kinds parser ───────
    let seen_from_store = parser.seen();
    assert_eq!(
        seen_from_store.len(),
        3,
        "all-kinds IngestParser must receive all 3 kind:1 events from cache-serve; \
         got {seen_from_store:?} — chirp-tui RawCacheIngestParser would miss feed notes on cold start",
    );
    assert!(
        seen_from_store.iter().all(|&k| k == 1),
        "all dispatched events must be kind:1; got {seen_from_store:?}",
    );
}

/// DEDUP GATE: an already-in-memory event (live-delivered then cache-served)
/// is skipped by cache-serve and does NOT reach the parser a second time.
///
/// This verifies the existing `events_cache.contains_key` dedup in
/// `serve_chunk` still fires correctly even when the parser is wired.
#[test]
fn cache_served_kind1_no_double_dispatch_when_already_in_memory() {
    let base_ts: u64 = 1_700_000_200;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.timeline_authors.insert(author.clone());

    let parser = CapturingIngestParser::new();
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.replace_range_parser(
            0..u32::MAX,
            "test.all-kinds",
            Arc::clone(&parser) as Arc<dyn IngestParser>,
        );
    }

    // Seed AND leave events in the in-memory cache (no cold restart).
    seed_events(&mut kernel, &keys, 2, base_ts);
    let seen_live = parser.seen().len();
    assert_eq!(seen_live, 2, "live ingest must fire parser twice; got {seen_live}");

    // Reset parser count before the cache-serve run.
    *parser.seen_kinds.lock().unwrap() = Vec::new();

    // Re-open interest (store completion key was cleared by … no restart, so we
    // need to clear it manually to force a re-serve attempt).
    kernel.clear_served_interest_shapes();
    open_author_interest(&mut kernel, 20, &author);
    drain_cache_serves(&mut kernel, 10);

    // Events are ALREADY in the events cache → serve_chunk dedup skips them.
    let seen_from_store = parser.seen();
    assert!(
        seen_from_store.is_empty(),
        "cache-serve must NOT re-dispatch already-in-memory events; got {seen_from_store:?}",
    );
}

// ─── ADR-0057 unified post-store projection on the cache-serve path ───────────
//
// These prove the unification codex required: cache-serve replay runs the SAME
// `Kernel::project_accepted_event` the live chokepoint runs, so the capability-
// cache transition sweep AND the D9 clamp fire on the cache-serve path too. The
// non-vacuity note on each: removing the shared-helper call from
// `feed_served_event` (or its transition sweep / clamp) fails the test.

/// Ingest an arbitrary signed event through the REAL live path (`handle_event`
/// → `verify_and_persist` → store + projection), assembling the wire `Value`
/// `serde_json::from_value::<NostrEvent>` expects (NostrEvent is not Serialize).
fn live_ingest(kernel: &mut Kernel, sub_id: &str, ev: &NostrEvent) {
    let value = serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "created_at": ev.created_at,
        "kind": ev.kind,
        "tags": ev.tags,
        "content": ev.content,
        "sig": ev.sig,
    });
    kernel.handle_event(RelayRole::Indexer, "wss://relay.test/", sub_id, &value);
}

/// Real signed kind:0 profile in `NostrEvent` shape with the given display name.
fn signed_kind0(keys: &::nostr::Keys, display_name: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Kind, Timestamp};
    let content = format!(r#"{{"display_name":"{display_name}"}}"#);
    let ev = EventBuilder::new(Kind::Metadata, content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

/// Open a kind:0 follow-feed interest for `author_hex` so the cache-serve path
/// will replay stored kind:0 events for that author.
fn open_kind0_interest(kernel: &mut Kernel, seed: u64, author_hex: &str) {
    let shape = InterestShape {
        authors: BTreeSet::from([author_hex.to_string()]),
        kinds: BTreeSet::from([0u32]),
        ..Default::default()
    };
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    kernel.open_interest_sub(sub_id(seed), interest);
}

fn profiles_ver(kernel: &Kernel) -> u64 {
    kernel
        .projection_rev_tracker
        .source_versions
        .get(crate::kernel::projection_rev::SRC_PROFILES)
}

/// (a) #1 FIX — a stored kind:0 served from the store on a cold restart bumps
/// `profiles_ver` (via the shared `project_accepted_event` transition sweep) AND
/// populates the capability profile cache, so incremental profile projections
/// re-emit instead of staying `Unchanged` (stale UI). Non-vacuous: deleting the
/// `project_accepted_event` call from `feed_served_event` leaves the cache empty
/// and `profiles_ver` unbumped — both assertions fail.
#[test]
fn cache_served_kind0_bumps_profiles_ver_and_populates_cache() {
    let base_ts: u64 = 1_700_000_300;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([0u32]);
    kernel.timeline_authors.insert(author.clone());

    // Phase 1: live-ingest a kind:0 into the store (also populates the cache).
    live_ingest(&mut kernel, "follow-feed-default", &signed_kind0(&keys, "Nova", base_ts));
    assert_eq!(
        kernel.profile_lookup().profile(&author).map(|p| p.display),
        Some("Nova".to_string()),
        "precondition: live kind:0 populated the profile cache",
    );

    // Phase 2: cold restart — clear in-memory caches AND the capability profile
    // cache, so the cache-serve replay is the ONLY thing that can repopulate it.
    simulate_cold_restart(&mut kernel);
    kernel
        .profile_lookup()
        .evict_to(&std::collections::HashSet::new(), 0);
    assert!(
        kernel.profile_lookup().profile(&author).is_none(),
        "profile cache cleared before cache-serve replay",
    );
    let ver_before = profiles_ver(&kernel);

    // Phase 3: replay via cache-serve with ZERO relays.
    open_kind0_interest(&mut kernel, 30, &author);
    drain_cache_serves(&mut kernel, 10);

    // Phase 4: the shared helper re-wrote the cache + bumped the rev.
    assert_eq!(
        kernel.profile_lookup().profile(&author).map(|p| p.display),
        Some("Nova".to_string()),
        "cache-served kind:0 must repopulate the capability profile cache via \
         the shared project_accepted_event → registered Kind0Parser",
    );
    assert!(
        profiles_ver(&kernel) > ver_before,
        "cache-served kind:0 must bump profiles_ver so incremental profile \
         projections re-emit after cold restart ({ver_before} -> {})",
        profiles_ver(&kernel),
    );
}

/// (b) PORTED from PR1b — a FUTURE-dated event served from the store on cold
/// restart is clamped to `now` in the observer fan-out (D9), via the SAME shared
/// helper. The store + read-cache retain the raw wire timestamp. Non-vacuous:
/// removing the clamp in `project_accepted_event` makes the observer see
/// NOW + 9_999.
#[test]
fn cache_served_future_dated_event_is_clamped_in_fan_out() {
    use crate::actor::{new_event_observer_slot, register_rust_observer, KernelEventObserver};
    use crate::kernel::clock::FixedClock;
    use crate::substrate::KernelEvent;
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime};

    struct CapturingObserver {
        seen: Mutex<HashMap<String, u64>>,
    }
    impl KernelEventObserver for CapturingObserver {
        fn on_kernel_event(&self, event: &KernelEvent) {
            self.seen
                .lock()
                .unwrap()
                .insert(event.id.clone(), event.created_at);
        }
    }

    const NOW_SECS: u64 = 1_700_000_000;
    let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(NOW_SECS);

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_clock(Arc::new(FixedClock(fixed)));

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([1u32]);
    kernel.timeline_authors.insert(author.clone());

    let future = signed_note(&keys, "from the future", NOW_SECS + 9_999);
    let past = signed_note(&keys, "from the past", NOW_SECS - 500_000);
    let future_id = future.id.clone();
    let past_id = past.id.clone();
    kernel.ingest_timeline_event(RelayRole::Content, "wss://seed.relay/", "follow-feed-default", future);
    kernel.ingest_timeline_event(RelayRole::Content, "wss://seed.relay/", "follow-feed-default", past);
    assert_eq!(kernel.events.len(), 2, "both seeded events in cache pre-restart");

    simulate_cold_restart(&mut kernel);
    assert!(kernel.events.is_empty(), "events cache empty after cold restart");

    // Observer registered AFTER restart → captures ONLY the cache-serve fan-out.
    let slot = new_event_observer_slot();
    let observer = Arc::new(CapturingObserver {
        seen: Mutex::new(HashMap::new()),
    });
    register_rust_observer(&slot, observer.clone());
    kernel.set_event_observers_handle(slot);

    kernel.sync_follow_feed_interests(&[author.clone()]);
    drain_cache_serves(&mut kernel, 4);

    let seen = observer.seen.lock().unwrap();
    assert_eq!(
        seen.get(&future_id).copied(),
        Some(NOW_SECS),
        "future-dated created_at served from the store must be clamped to now in \
         the observer fan-out (D9, via the shared project_accepted_event)",
    );
    assert_eq!(
        seen.get(&past_id).copied(),
        Some(NOW_SECS - 500_000),
        "past-dated created_at passes through unchanged — clamp is min(wire, now)",
    );
    drop(seen);

    assert_eq!(
        kernel.events[future_id.as_str()].created_at,
        NOW_SECS + 9_999,
        "the served StoredEvent retains the unclamped wire created_at; only the \
         observer payload is clamped",
    );
}

/// (c) KIND-AGNOSTIC — cache-served kind:10002 (mailbox) and kind:10050 (DM)
/// fire their transitions via the SAME shared helper: the registered parser
/// writes the cache and the transition sweep fires `on_mailbox_changed` /
/// `on_dm_relays_changed`. Proven by the resulting cache state populated purely
/// from cache-serve replay (the parser ran via `project_accepted_event`).
/// Non-vacuous: dropping the shared-helper call leaves both caches empty.
#[test]
fn cache_served_replaceables_fire_transitions_kind_agnostically() {
    use crate::substrate::IngestParser;

    let base_ts: u64 = 1_700_000_400;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([0u32, 10_002u32]);
    kernel.timeline_authors.insert(author.clone());

    // Register a kind:10002 parser writing a real mailbox cache, mirroring
    // production composition (the kernel's test-default kind:0 parser is already
    // registered). Use the same in-memory mailbox cache the kernel reads.
    let mailbox = kernel.mailbox_cache_arc();
    let mailbox_parser: Arc<dyn IngestParser> =
        Arc::new(TestKind10002Parser { cache: mailbox });
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.register_kind(10_002, mailbox_parser);
    }

    // Phase 1: live-ingest a kind:0 and a kind:10002 for the author.
    live_ingest(&mut kernel, "follow-feed-default", &signed_kind0(&keys, "Nova", base_ts));
    live_ingest(&mut kernel, "follow-feed-default", &signed_kind10002(&keys, "wss://write.relay/", base_ts));
    assert!(kernel.profile_lookup().contains(&author), "precondition: profile cached");
    assert!(kernel.mailbox_cache().known(&author), "precondition: mailbox cached");

    // Phase 2: cold restart + clear both capability caches so cache-serve replay
    // is the only repopulation path.
    simulate_cold_restart(&mut kernel);
    kernel.profile_lookup().evict_to(&std::collections::HashSet::new(), 0);
    kernel.mailbox_cache().remove(&author);
    assert!(!kernel.profile_lookup().contains(&author), "profile cleared pre-replay");
    assert!(!kernel.mailbox_cache().known(&author), "mailbox cleared pre-replay");

    // Phase 3: replay both kinds via cache-serve.
    open_kind0_interest(&mut kernel, 40, &author);
    open_kind10002_interest(&mut kernel, 41, &author);
    drain_cache_serves(&mut kernel, 20);

    // Phase 4: BOTH capability caches were repopulated by the SAME shared
    // helper's parser dispatch — kind-agnostic, no per-kind cache-serve code.
    assert!(
        kernel.profile_lookup().contains(&author),
        "cache-served kind:0 repopulated the profile cache via the shared helper",
    );
    assert!(
        kernel.mailbox_cache().known(&author),
        "cache-served kind:10002 repopulated the mailbox cache via the shared helper",
    );
}

/// Real signed kind:10002 (NIP-65) in `NostrEvent` shape with one write relay.
fn signed_kind10002(keys: &::nostr::Keys, write_relay: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Kind, Timestamp};
    let ev = EventBuilder::new(Kind::RelayList, "")
        .tags([::nostr::Tag::parse(["r", write_relay, "write"]).expect("valid r tag")])
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

fn open_kind10002_interest(kernel: &mut Kernel, seed: u64, author_hex: &str) {
    let shape = InterestShape {
        authors: BTreeSet::from([author_hex.to_string()]),
        kinds: BTreeSet::from([10_002u32]),
        ..Default::default()
    };
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    kernel.open_interest_sub(sub_id(seed), interest);
}

/// Minimal kind:10002 ingest parser writing the substrate mailbox cache —
/// mirrors `nmp_router::Kind10002Parser` (which `nmp-core` cannot depend on).
struct TestKind10002Parser {
    cache: Arc<dyn crate::substrate::MailboxCache>,
}

impl crate::substrate::IngestParser for TestKind10002Parser {
    fn parse(&self, evt: &crate::store::VerifiedEvent) {
        let raw = evt.raw();
        if raw.kind != 10_002 {
            return;
        }
        let parsed = super::parse_relay_list_to_substrate(&raw.id, raw.created_at, &raw.tags);
        let empty = parsed.read.is_empty() && parsed.write.is_empty() && parsed.both.is_empty();
        if empty {
            self.cache.remove(&raw.pubkey);
        } else {
            self.cache.upsert(raw.pubkey.clone(), parsed);
        }
    }
}
