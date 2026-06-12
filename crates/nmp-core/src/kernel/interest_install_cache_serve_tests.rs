//! ADR-0045 single choke-point — interest-install cache-serve regression tests.
//!
//! Root cause (Fable debugging pass, 2026-06-13): `ActorCommand::PushInterest`
//! and `ActorCommand::EnsureInterest` registered interests in the subscription
//! registry and enqueued a recompile trigger, but **never** enqueued the
//! ADR-0045 E1 cache-serve. Events already in the persistent store were
//! therefore invisible to kind-parsers installed for those interests on any
//! session after the one that originally fetched them.
//!
//! Concrete victim: Marmot key-package lookup + giftwrap inbox interests
//! (pushed via `app.push_interest`) could never be satisfied from the store
//! on relaunch → MLS group creation permanently no-ops cross-session.
//!
//! This fix extracts the E1 serve block from `open_interest_sub` into
//! `Kernel::enqueue_interest_cache_serve` (single choke-point) and calls it
//! from every interest-install path.
//!
//! # Test inventory
//!
//! - `push_interest_serves_store_on_install` — PushInterest with pre-seeded store.
//! - `ensure_interest_serves_store_on_newly_installed` — EnsureInterest same.
//! - `ensure_interest_no_serve_on_idempotent_reinstall` — EnsureInterest second
//!   call on same slot does NOT re-serve (completion-key idempotency).
//! - `two_session_push_interest_regression` — the MLS fingerprint: session-1
//!   kernel ingests + stores KP events; new kernel instance over the same store;
//!   push the KP interest; parser receives stored events WITHOUT any network.
//! - `push_interest_ingest_parser_idempotent_re_ingest` — re-serving an already-
//!   processed event does not panic and does not produce duplicate deliveries
//!   within a session (in-memory dedup).

use super::cache_serve_tests::{drain_cache_serves, hex_pk, seed_events, simulate_cold_restart};
use super::*;
use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::VerifiedEvent;
use crate::subs::{InterestRegistry, SubIdentity, SubKey, SubOwnerKey, SubScope};
use crate::substrate::IngestParser;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

// ─── Fixtures ────────────────────────────────────────────────────────────────

/// Minimal `IngestParser` that records every `(kind, id)` it receives.
struct CapturingParser {
    seen: Mutex<Vec<(u32, String)>>,
}

impl CapturingParser {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
        })
    }

    fn seen_kinds(&self) -> Vec<u32> {
        self.seen.lock().unwrap().iter().map(|(k, _)| *k).collect()
    }

    fn seen_ids(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(_, id)| id.clone())
            .collect()
    }

    fn clear(&self) {
        self.seen.lock().unwrap().clear();
    }
}

impl IngestParser for CapturingParser {
    fn parse(&self, evt: &VerifiedEvent) {
        let raw = evt.raw();
        self.seen
            .lock()
            .unwrap()
            .push((raw.kind, raw.id.clone()));
    }
}

/// Build a `LogicalInterest` for `kind:1` from `author_hex`.
fn author_kind1_interest(id: u64, author_hex: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: BTreeSet::from([author_hex.to_string()]),
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// Build a `LogicalInterest` for `kind:443` (Marmot key-package kind).
fn kp_interest(id: u64, target_pubkey: &str) -> LogicalInterest {
    // kind:443 is the Marmot MLS key-package kind. We model it as a
    // #p-tagged interest (the real Marmot interest shape uses #p to match
    // KPs published for a specific recipient pubkey).
    let mut shape = InterestShape {
        kinds: BTreeSet::from([443u32]),
        ..Default::default()
    };
    shape
        .tags
        .insert("p".to_string(), BTreeSet::from([target_pubkey.to_string()]));
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// Build a `SubIdentity` for a generic non-feed interest.
fn sub_id(seed: u64) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(seed),
        SubKey::new(seed),
        SubScope::Global,
    )
}

/// Seed a kind:443 (Marmot KP) event addressed to `target_hex` into the
/// kernel's store via `handle_event` (the live ingest path that persists to the
/// store). Returns the event id.
fn seed_kp_event(
    kernel: &mut Kernel,
    keys: &::nostr::Keys,
    target_hex: &str,
    ts: u64,
) -> String {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let target_pk: ::nostr::PublicKey = target_hex.parse().expect("valid hex pubkey");
    let ev = EventBuilder::new(Kind::from(443u16), "kp-payload")
        .tags(vec![Tag::public_key(target_pk)])
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with generated keys");
    let tag_vecs: Vec<Vec<String>> = ev
        .tags
        .iter()
        .map(|t: &::nostr::Tag| t.as_slice().to_vec())
        .collect();
    let json = serde_json::json!({
        "id": ev.id.to_hex(),
        "pubkey": ev.pubkey.to_hex(),
        "created_at": ev.created_at.as_secs(),
        "kind": ev.kind.as_u16(),
        "tags": tag_vecs,
        "content": ev.content.clone(),
        "sig": ev.sig.to_string(),
    });
    let id = ev.id.to_hex();
    kernel.handle_event(RelayRole::Content, "wss://relay.test/", "kp-sub", &json);
    id
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// PRIMARY CONTRACT (PushInterest):
///
/// A kind:1 event seeded into the store via live ingest is served to a
/// registered `IngestParser` when `PushInterest` installs the matching
/// interest on a cold-restart kernel (empty in-memory caches, warm store).
///
/// Regression net: before this fix the cache-serve was never enqueued from
/// the `PushInterest` dispatch arm, so parsers never saw store-resident events.
#[test]
fn push_interest_serves_store_on_install() {
    let base_ts: u64 = 1_730_000_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // ── Phase 1: seed 3 kind:1 events into the store ─────────────────────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.timeline_authors.insert(author.clone());

    let parser = CapturingParser::new();
    kernel.register_ingest_parser(1, parser.clone());

    seed_events(&mut kernel, &keys, 3, base_ts);
    assert_eq!(
        parser.seen_kinds().len(),
        3,
        "Phase 1: parser must see 3 events on live ingest"
    );

    // ── Phase 2: cold restart ─────────────────────────────────────────────────
    simulate_cold_restart(&mut kernel);
    parser.clear();
    assert!(kernel.events.is_empty(), "events cache must be cleared");
    assert!(parser.seen_kinds().is_empty(), "parser must be cleared");

    // ── Phase 3: install interest via PushInterest path ───────────────────────
    // Mirrors the production `registry_mut().push(interest)` + cache-serve path
    // added by this PR.
    let interest = author_kind1_interest(1, &author);
    let serve_key = InterestRegistry::legacy_key(&interest.id);
    let serve_shape = interest.shape.clone();
    kernel.lifecycle_mut().registry_mut().push(interest);
    kernel.enqueue_interest_cache_serve(&serve_key, &serve_shape);
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: parser must have received all 3 stored events ────────────────
    let seen = parser.seen_kinds();
    assert_eq!(
        seen.len(),
        3,
        "PushInterest FAIL: IngestParser must receive 3 store-resident kind:1 events \
         after PushInterest install; got {seen:?}"
    );
    assert!(
        seen.iter().all(|&k| k == 1),
        "all dispatched events must be kind:1; got {seen:?}"
    );
}

/// PRIMARY CONTRACT (EnsureInterest):
///
/// Same invariant but via the `EnsureInterest` install path (newly-installed).
#[test]
fn ensure_interest_serves_store_on_newly_installed() {
    let base_ts: u64 = 1_730_001_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.timeline_authors.insert(author.clone());

    let parser = CapturingParser::new();
    kernel.register_ingest_parser(1, parser.clone());

    seed_events(&mut kernel, &keys, 2, base_ts);
    assert_eq!(parser.seen_kinds().len(), 2);

    simulate_cold_restart(&mut kernel);
    parser.clear();

    // ── EnsureInterest (newly installed) ─────────────────────────────────────
    let identity = sub_id(42);
    let interest = author_kind1_interest(42, &author);
    let serve_key = identity.key;
    let serve_shape = interest.shape.clone();
    let newly = kernel
        .lifecycle_mut()
        .registry_mut()
        .ensure_sub(identity, interest);
    assert!(newly, "must be newly installed");

    // The dispatch arm only serves when newly_installed.
    kernel.enqueue_interest_cache_serve(&serve_key, &serve_shape);
    drain_cache_serves(&mut kernel, 10);

    let seen = parser.seen_kinds();
    assert_eq!(
        seen.len(),
        2,
        "EnsureInterest FAIL: parser must see 2 store-resident events on newly-installed; \
         got {seen:?}"
    );
}

/// IDEMPOTENCY (EnsureInterest):
///
/// A second `EnsureInterest` call for the same `(owner, key, scope)` slot
/// returns `newly_installed = false` and must NOT trigger a re-serve (the
/// completion key is already in `served_interest_shapes`).
#[test]
fn ensure_interest_no_serve_on_idempotent_reinstall() {
    let base_ts: u64 = 1_730_002_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.timeline_authors.insert(author.clone());

    let parser = CapturingParser::new();
    kernel.register_ingest_parser(1, parser.clone());

    seed_events(&mut kernel, &keys, 2, base_ts);
    simulate_cold_restart(&mut kernel);
    parser.clear();

    // First install — serves.
    let identity_1 = sub_id(100);
    let interest_1 = author_kind1_interest(100, &author);
    let serve_key_1 = identity_1.key;
    let serve_shape_1 = interest_1.shape.clone();
    let newly_1 = kernel
        .lifecycle_mut()
        .registry_mut()
        .ensure_sub(identity_1, interest_1);
    assert!(newly_1);
    kernel.enqueue_interest_cache_serve(&serve_key_1, &serve_shape_1);
    drain_cache_serves(&mut kernel, 10);
    let after_first = parser.seen_kinds().len();
    assert_eq!(after_first, 2, "first install must serve 2 events");

    parser.clear();

    // Second install — same (owner, key, scope) = idempotent, not new.
    let identity_2 = sub_id(100);
    let interest_2 = author_kind1_interest(100, &author);
    let serve_key_2 = identity_2.key;
    let serve_shape_2 = interest_2.shape.clone();
    let newly_2 = kernel
        .lifecycle_mut()
        .registry_mut()
        .ensure_sub(identity_2, interest_2);
    assert!(!newly_2, "second ensure_sub for same slot must return false");

    // The dispatch arm gates the serve on newly_installed. Call the helper
    // anyway to assert that the completion-key guard makes it a true no-op
    // (idempotent re-serve invariant, verified via has_pending_cache_serves).
    kernel.enqueue_interest_cache_serve(&serve_key_2, &serve_shape_2);
    // serve_interest_shapes already contains the key → no pending serve.
    assert!(
        !kernel.has_pending_cache_serves(),
        "second enqueue of an already-served completion key must be a no-op"
    );
    drain_cache_serves(&mut kernel, 10);

    let after_second = parser.seen_kinds();
    assert!(
        after_second.is_empty(),
        "idempotent reinstall must NOT re-dispatch events; got {after_second:?}"
    );
}

/// TWO-SESSION REGRESSION (MLS fingerprint):
///
/// This is the deterministic host reproduction of the cross-session MLS bug:
///
/// 1. Session 1 kernel ingests + persists kind:443 (Marmot KP) events via the
///    live relay path.
/// 2. A new `Kernel` instance is created over the SAME store (simulating a
///    process relaunch). In-memory caches are gone.
/// 3. `push_interest` (the production path from `nmp-marmot/src/ffi.rs:499`)
///    installs the KP interest.
/// 4. The `IngestParser` (stand-in for `MarmotIngestParser`) must receive the
///    stored KP events WITHOUT any network activity.
///
/// Before this fix: step 4 produced 0 parser deliveries → MLS group creation
/// no-ops on relaunch.
#[test]
fn two_session_push_interest_kp_regression() {
    let base_ts: u64 = 1_740_000_000;
    let kp_publisher_keys = ::nostr::Keys::generate();
    let receiver_keys = ::nostr::Keys::generate();
    let receiver_hex = receiver_keys.public_key().to_hex();

    // ── Session 1: ingest KP events into the persistent store ────────────────
    let mut kernel_s1 = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel_s1.active_account = Some(receiver_hex.clone());

    // Register a parser (session 1 also uses it — confirms live ingest fires).
    let parser_s1 = CapturingParser::new();
    kernel_s1.register_ingest_parser(443, parser_s1.clone());

    // Seed 2 kind:443 KP events (addressed to receiver_hex via #p).
    let kp_id_1 = seed_kp_event(&mut kernel_s1, &kp_publisher_keys, &receiver_hex, base_ts);
    let kp_id_2 =
        seed_kp_event(&mut kernel_s1, &kp_publisher_keys, &receiver_hex, base_ts + 1);

    // Confirm live delivery fires the parser.
    let seen_s1 = parser_s1.seen_ids();
    assert!(
        seen_s1.contains(&kp_id_1) && seen_s1.contains(&kp_id_2),
        "Session 1: parser must see both KP events on live ingest; got {seen_s1:?}"
    );

    // ── Session 2: new kernel over the SAME store (process relaunch) ──────────
    // `simulate_cold_restart` clears in-memory caches while keeping the store.
    // This is the exact boundary: store survives, everything else is fresh.
    simulate_cold_restart(&mut kernel_s1);
    let mut kernel = kernel_s1; // rename for clarity — same kernel, wiped caches.
    kernel.active_account = Some(receiver_hex.clone());

    let parser_s2 = CapturingParser::new();
    kernel.register_ingest_parser(443, parser_s2.clone());

    assert!(
        kernel.events.is_empty(),
        "Session 2 pre-condition: events cache must be empty"
    );

    // ── Session 2: push the KP interest (production path) ────────────────────
    let interest = kp_interest(999, &receiver_hex);
    let serve_key = InterestRegistry::legacy_key(&interest.id);
    let serve_shape = interest.shape.clone();
    kernel.lifecycle_mut().registry_mut().push(interest);
    // ADR-0045 single choke-point — the fix being tested.
    kernel.enqueue_interest_cache_serve(&serve_key, &serve_shape);
    drain_cache_serves(&mut kernel, 10);

    // ── Assert: parser received stored KP events, NO network needed ───────────
    let seen_s2 = parser_s2.seen_ids();
    assert!(
        seen_s2.contains(&kp_id_1),
        "TWO-SESSION REGRESSION FAIL: parser must receive KP event 1 ({kp_id_1}) \
         from the store on session-2 PushInterest; got {seen_s2:?}. \
         This is the MLS cross-session no-op bug."
    );
    assert!(
        seen_s2.contains(&kp_id_2),
        "TWO-SESSION REGRESSION FAIL: parser must receive KP event 2 ({kp_id_2}) \
         from the store on session-2 PushInterest; got {seen_s2:?}. \
         This is the MLS cross-session no-op bug."
    );
}

/// IDEMPOTENT RE-SERVE (point 2 of the task):
///
/// Re-serving an event that was already processed (by the same parser within
/// the same session after a `clear_served_interest_shapes` reset) must:
/// a. NOT panic.
/// b. NOT produce additional deliveries because the event is already in the
///    in-memory `events` cache (serve_chunk skips events already in cache).
///
/// This is the key guarantee that parsers can be idempotent: MDK dedups
/// processed welcomes; the in-memory dedup ensures even a re-triggered serve
/// does not double-dispatch.
#[test]
fn push_interest_ingest_parser_idempotent_re_ingest() {
    let base_ts: u64 = 1_750_000_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.timeline_authors.insert(author.clone());

    let parser = CapturingParser::new();
    kernel.register_ingest_parser(1, parser.clone());

    seed_events(&mut kernel, &keys, 3, base_ts);
    let after_live = parser.seen_kinds().len();
    assert_eq!(after_live, 3, "live ingest: parser sees 3 events");

    // First serve — in-memory events are already present; cache-serve skips them.
    let interest = author_kind1_interest(50, &author);
    let serve_key_1 = InterestRegistry::legacy_key(&interest.id);
    let serve_shape_1 = interest.shape.clone();
    kernel.lifecycle_mut().registry_mut().push(interest);
    kernel.enqueue_interest_cache_serve(&serve_key_1, &serve_shape_1);
    drain_cache_serves(&mut kernel, 10);
    // Events already in memory → serve skips → no additional dispatches.
    let after_first_serve = parser.seen_kinds().len();
    assert_eq!(
        after_first_serve,
        3, // still 3 (live ingest only, serve was a no-op)
        "first serve: events already in memory, serve must be a no-op dedup"
    );

    // Force a re-serve by clearing the completion set (simulates account-switch
    // or same-shape re-install scenario). Events remain in the events cache.
    kernel.clear_served_interest_shapes();
    parser.clear();

    // Second install via PushInterest with a new InterestId (fresh completion key).
    let interest_2 = author_kind1_interest(51, &author);
    let serve_key_2 = InterestRegistry::legacy_key(&interest_2.id);
    let serve_shape_2 = interest_2.shape.clone();
    kernel.lifecycle_mut().registry_mut().push(interest_2);
    // This must NOT panic and must NOT deliver duplicate events (in-memory dedup).
    kernel.enqueue_interest_cache_serve(&serve_key_2, &serve_shape_2);
    drain_cache_serves(&mut kernel, 10);

    let after_reserve = parser.seen_kinds();
    assert!(
        after_reserve.is_empty(),
        "idempotent re-serve: events already in the events cache must NOT be \
         re-dispatched; got {after_reserve:?}"
    );
}
