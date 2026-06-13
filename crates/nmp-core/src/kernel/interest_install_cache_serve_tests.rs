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

use super::cache_serve_tests::{drain_cache_serves, seed_events, simulate_cold_restart};
use super::interest_install_cache_serve_support::{
    author_kind1_interest, kp_interest, seed_kind0_event, seed_kp_event, sub_id, CapturingParser,
};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

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

    // ── Phase 3: install interest via the real PushInterest front door ────────
    // `push_interest_and_serve` is exactly what the `ActorCommand::PushInterest`
    // dispatch arm calls — registry push + recompile trigger + store-cache serve.
    let interest = author_kind1_interest(1, &author);
    kernel.push_interest_and_serve(interest);
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

    // ── EnsureInterest (newly installed) via the real front door ─────────────
    // `ensure_interest_and_serve` is exactly what the
    // `ActorCommand::EnsureInterest` dispatch arm (and open_interest_sub /
    // open_uri) call — ensure_sub + trigger + serve, all gated on newly-installed.
    let identity = sub_id(42);
    let interest = author_kind1_interest(42, &author);
    let newly = kernel.ensure_interest_and_serve(identity, interest, "ensure-interest");
    assert!(newly, "must be newly installed");
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

    // First install — serves (via the real front door).
    let identity_1 = sub_id(100);
    let interest_1 = author_kind1_interest(100, &author);
    let newly_1 = kernel.ensure_interest_and_serve(identity_1, interest_1, "ensure-interest");
    assert!(newly_1);
    drain_cache_serves(&mut kernel, 10);
    let after_first = parser.seen_kinds().len();
    assert_eq!(after_first, 2, "first install must serve 2 events");

    parser.clear();

    // Second install — same (owner, key, scope) = idempotent, not new. The
    // front door returns false and internally skips both the trigger and the
    // serve; no pending serve is queued.
    let identity_2 = sub_id(100);
    let interest_2 = author_kind1_interest(100, &author);
    let newly_2 = kernel.ensure_interest_and_serve(identity_2, interest_2, "ensure-interest");
    assert!(
        !newly_2,
        "second ensure_interest_and_serve for same slot must return false"
    );
    assert!(
        !kernel.has_pending_cache_serves(),
        "idempotent reinstall must not queue a serve"
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
    let kp_id_2 = seed_kp_event(
        &mut kernel_s1,
        &kp_publisher_keys,
        &receiver_hex,
        base_ts + 1,
    );

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
    // `push_interest_and_serve` is the exact call the
    // `ActorCommand::PushInterest` arm makes from `nmp-marmot/src/ffi.rs:499`.
    let interest = kp_interest(999, &receiver_hex);
    kernel.push_interest_and_serve(interest);
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
    kernel.push_interest_and_serve(interest);
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
    // This must NOT panic and must NOT deliver duplicate events (in-memory dedup).
    let interest_2 = author_kind1_interest(51, &author);
    kernel.push_interest_and_serve(interest_2);
    drain_cache_serves(&mut kernel, 10);

    let after_reserve = parser.seen_kinds();
    assert!(
        after_reserve.is_empty(),
        "idempotent re-serve: events already in the events cache must NOT be \
         re-dispatched; got {after_reserve:?}"
    );
}

/// OPEN-URI BYPASS REGRESSION (PR #1237 review F2):
///
/// Opening a `nostr:` URI installs an interest for the resolved target. Before
/// the F2 fix, `open_uri` called bare `ensure_sub` with neither a recompile
/// trigger nor a store-cache serve — so a `nostr:npub…` whose kind:0 metadata
/// was already in the store would NOT surface those stored events to parsers.
///
/// This test seeds a kind:0 event into the store, cold-restarts (store warm,
/// caches cold), then drives the real `dispatch_kernel_action(OpenUri{npub})`
/// path and asserts the registered kind:0 parser receives the stored event
/// WITHOUT any network — proving open_uri now routes through the single
/// ensure-install front door (`ensure_interest_and_serve`).
#[test]
fn open_uri_serves_store_for_resolved_target() {
    use crate::app::{KernelAction, KernelUpdate};
    use crate::kernel_action::dispatch_kernel_action;
    use crate::nip19::encode_npub;

    let base_ts: u64 = 1_760_000_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // ── Phase 1: seed a kind:0 metadata event into the store ─────────────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parser = CapturingParser::new();
    kernel.register_ingest_parser(0, parser.clone());
    let meta_id = seed_kind0_event(&mut kernel, &keys, base_ts);
    assert!(
        parser.seen_ids().contains(&meta_id),
        "Phase 1: parser must see the kind:0 event on live ingest"
    );

    // ── Phase 2: cold restart (store warm, caches cold) ──────────────────────
    simulate_cold_restart(&mut kernel);
    parser.clear();
    assert!(kernel.events.is_empty());

    // ── Phase 3: open the npub via the real action dispatcher ────────────────
    let npub = encode_npub(&author).expect("valid npub");
    let update = dispatch_kernel_action(
        &mut kernel,
        KernelAction::OpenUri {
            uri: format!("nostr:{npub}"),
        },
    );
    assert!(
        matches!(update, KernelUpdate::ViewOpened { .. }),
        "open_uri must resolve the npub to a profile view; got {update:?}"
    );
    // open_uri serves synchronously through enqueue_interest_cache_serve; drain
    // any continuation to be safe.
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: parser must have received the stored kind:0 event ───────────
    assert!(
        parser.seen_ids().contains(&meta_id),
        "OPEN-URI BYPASS FAIL: parser must receive the store-resident kind:0 \
         event ({meta_id}) after open_uri installs the profile interest; \
         got {:?}",
        parser.seen_ids()
    );
}
