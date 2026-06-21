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
//! This fix funnels all interest-install paths through
//! [`crate::kernel::Kernel::register_interest`] (single front-door), which
//! enqueues the ADR-0045 E1 store-cache serve for every newly-installed
//! interest.
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
    // `register_interest(Replace)` is exactly what the `ActorCommand::PushInterest`
    // dispatch arm calls — unified front-door with Replace policy.
    let interest = author_kind1_interest(1, &author);
    {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        use crate::subs::SubIdentity;
        let identity = SubIdentity::from_legacy_interest(&interest);
        kernel.register_interest(&[InterestRegistration { identity, interest, policy: InterestWrite::Replace }], "push-interest");
    }
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
    // `register_interest(EnsureAbsent)` is exactly what the
    // `ActorCommand::EnsureInterest` dispatch arm (and open_interest_sub /
    // open_uri) calls — unified front-door with EnsureAbsent policy.
    let identity = sub_id(42);
    let interest = author_kind1_interest(42, &author);
    let newly = {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        let outcomes = kernel.register_interest(&[InterestRegistration { identity, interest, policy: InterestWrite::EnsureAbsent }], "ensure-interest");
        outcomes[0].newly_installed
    };
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
    let newly_1 = {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        let outcomes = kernel.register_interest(&[InterestRegistration { identity: identity_1, interest: interest_1, policy: InterestWrite::EnsureAbsent }], "ensure-interest");
        outcomes[0].newly_installed
    };
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
    let newly_2 = {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        let outcomes = kernel.register_interest(&[InterestRegistration { identity: identity_2, interest: interest_2, policy: InterestWrite::EnsureAbsent }], "ensure-interest");
        outcomes[0].newly_installed
    };
    assert!(
        !newly_2,
        "second EnsureAbsent for same slot must return false (not newly installed)"
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
    // `register_interest(Replace)` is the exact call the
    // `ActorCommand::PushInterest` arm makes (Replace policy).
    let interest = kp_interest(999, &receiver_hex);
    {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        use crate::subs::SubIdentity;
        let identity = SubIdentity::from_legacy_interest(&interest);
        kernel.register_interest(&[InterestRegistration { identity, interest, policy: InterestWrite::Replace }], "push-interest");
    }
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
    {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        use crate::subs::SubIdentity;
        let identity = SubIdentity::from_legacy_interest(&interest);
        kernel.register_interest(&[InterestRegistration { identity, interest, policy: InterestWrite::Replace }], "push-interest");
    }
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
    {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        use crate::subs::SubIdentity;
        let identity = SubIdentity::from_legacy_interest(&interest_2);
        kernel.register_interest(&[InterestRegistration { identity, interest: interest_2, policy: InterestWrite::Replace }], "push-interest");
    }
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
/// ensure-install front door (`register_interest(EnsureAbsent)`).
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
    // open_uri serves synchronously through register_interest; drain any
    // continuation to be safe.
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

/// PROFILE-CLAIM STORE-FIRST (Phase C regression):
///
/// `register_profile_claim_interest` routes through the unified front-door
/// (`register_interest` with Replace policy). This test proves that a stored
/// kind:0 metadata event populates the ProfileCache on a cold-cache kernel
/// immediately after `claim_profile` — no relay delivery needed.
///
/// Regression guard: before Phase C the profile-claim path called bare
/// `set_sub` without a cache-serve enqueue, so a stored kind:0 was invisible
/// on relaunch (the "timeline shows only pubkeys after relaunch" bug).
///
/// Setup: after seeding, the profile_lookup is swapped to a FRESH empty
/// TestProfileCache (+ matching TestKind0Parser) so `profile_lookup().profile(P)`
/// is genuinely None before the claim — faithfully simulating a process restart
/// where the in-memory ProfileCache is empty but the on-disk store is warm.
#[test]
fn profile_claim_serves_stored_kind0_from_store_on_cold_cache() {
    use crate::kernel::refs::{ProfileShape, RefLiveness, RefNamespace, RefShape};
    use crate::substrate::{ProfileLookup, TestKind0Parser, TestProfileCache};

    let base_ts: u64 = 1_770_000_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // ── Phase 1: seed a kind:0 event into the store via live ingest ──────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let meta_id = seed_kind0_event(&mut kernel, &keys, base_ts);

    // ── Phase 2: cold restart — clear in-memory event caches ─────────────────
    simulate_cold_restart(&mut kernel);
    assert!(kernel.events.is_empty(), "events cache must be empty after restart");

    // Swap to a fresh, empty ProfileCache — faithfully simulates process restart
    // where the in-memory profile lookup is cold (the store is warm).
    let cold_cache = std::sync::Arc::new(TestProfileCache::new());
    kernel.set_profile_lookup(std::sync::Arc::clone(&cold_cache) as std::sync::Arc<dyn ProfileLookup>);
    // Register a matching kind:0 parser that writes INTO the new cold cache,
    // so that cache-serve events populate it (the live pipeline is unchanged).
    kernel.register_ingest_parser(0, std::sync::Arc::new(TestKind0Parser::new(std::sync::Arc::clone(&cold_cache))));

    // Pre-condition: cold cache is genuinely empty.
    assert!(
        cold_cache.profile(&author).is_none(),
        "Pre-condition: cold profile cache must not contain the author"
    );

    // ── Phase 3: claim the profile — routes through register_interest(Replace) ─
    // cold_cache.contains(&author) == false → want_register = true → front-door
    // fires, cache-serve enqueued. No relay connected, no wire event injected.
    kernel.resolve_ref(
        RefNamespace::Profile,
        author.clone(),
        "test-consumer".to_string(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk.into(),
        false,
        Vec::new(),
    );
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: cold_cache must now have P — served from the store ───────────
    assert!(
        cold_cache.profile(&author).is_some(),
        "PROFILE-CLAIM STORE-FIRST FAIL: profile_lookup().profile(P) must be Some \
         after claim_profile installs the kind:0 interest and the cache-serve \
         runs from the store; got None. This is the cold-cache kind:0 bug \
         (timeline shows only pubkeys after relaunch)."
    );
}
