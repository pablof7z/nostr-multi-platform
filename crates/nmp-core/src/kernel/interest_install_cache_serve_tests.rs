//! ADR-0070 single choke-point — interest-install cache-serve regression tests.
//!
//! Root cause (Fable debugging pass, 2026-06-13): interest installs registered
//! entries in the subscription registry and enqueued a recompile trigger, but
//! **never** enqueued the
//! ADR-0070 E1 cache-serve. Events already in the persistent store were
//! therefore invisible to kind-parsers installed for those interests on any
//! session after the one that originally fetched them.
//!
//! Concrete victim: Marmot key-package lookup + giftwrap inbox interests
//! (installed via `app.ensure_interest`) could never be satisfied from the store
//! on relaunch → MLS group creation permanently no-ops cross-session.
//!
//! This fix funnels all interest-install paths through
//! [`crate::kernel::Kernel::register_interest`] (single front-door), which
//! enqueues the ADR-0070 E1 store-cache serve for every newly-installed
//! interest.
//!
//! # Test inventory
//!
//! - `replace_interest_serves_store_on_install` — replace install with pre-seeded store.
//! - `ensure_interest_serves_store_on_newly_installed` — EnsureInterest same.
//! - `ensure_interest_no_serve_on_idempotent_reinstall` — EnsureInterest second
//!   call on same slot does NOT re-serve (completion-key idempotency).
//! - `two_session_interest_install_regression` — the MLS fingerprint: session-1
//!   kernel ingests + stores KP events; new kernel instance over the same store;
//!   install the KP interest; parser receives stored events WITHOUT any network.
//! - `interest_install_ingest_parser_idempotent_re_ingest` — re-serving an already-
//!   processed event does not panic and does not produce duplicate deliveries
//!   within a session (in-memory dedup).

use super::cache_serve_tests::{drain_cache_serves, seed_events, simulate_cold_restart};
use super::interest_install_cache_serve_support::{
    author_kind1_interest, kp_interest, seed_kp_event, sub_id, CapturingParser,
};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

// ─── Tests ───────────────────────────────────────────────────────────────────

/// PRIMARY CONTRACT (EnsureInterest):
///
/// A kind:1 event seeded into the store via live ingest is served to a
/// registered `IngestParser` when `EnsureInterest` installs the matching
/// interest on a cold-restart kernel (empty in-memory caches, warm store).
///
/// Regression net: before this fix the cache-serve was never enqueued from
/// the `EnsureInterest` dispatch arm, so parsers never saw store-resident events.
#[test]
fn replace_interest_serves_store_on_install() {
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

    // ── Phase 3: install interest via the real EnsureInterest front door ────────
    // `register_interest(Replace)` is exactly what the `InterestsCommand::EnsureInterest`
    // dispatch arm calls — unified front-door with Replace policy.
    let interest = author_kind1_interest(1, &author);
    {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        let identity = crate::subs::test_identity_for_interest(
            ("scoped-test-interest", interest.id.0),
            &interest,
        );
        kernel.register_interest(
            &[InterestRegistration {
                identity,
                interest,
                policy: InterestWrite::Replace,
            }],
            "replace-interest",
        );
    }
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: parser must have received all 3 stored events ────────────────
    let seen = parser.seen_kinds();
    assert_eq!(
        seen.len(),
        3,
        "EnsureInterest FAIL: IngestParser must receive 3 store-resident kind:1 events \
         after EnsureInterest install; got {seen:?}"
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
    // `InterestsCommand::EnsureInterest` dispatch arm (and open_interest_sub /
    // open_uri) calls — unified front-door with EnsureAbsent policy.
    let identity = sub_id(42);
    let interest = author_kind1_interest(42, &author);
    let newly = {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        let outcomes = kernel.register_interest(
            &[InterestRegistration {
                identity,
                interest,
                policy: InterestWrite::EnsureAbsent,
            }],
            "ensure-interest",
        );
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
        let outcomes = kernel.register_interest(
            &[InterestRegistration {
                identity: identity_1,
                interest: interest_1,
                policy: InterestWrite::EnsureAbsent,
            }],
            "ensure-interest",
        );
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
        let outcomes = kernel.register_interest(
            &[InterestRegistration {
                identity: identity_2,
                interest: interest_2,
                policy: InterestWrite::EnsureAbsent,
            }],
            "ensure-interest",
        );
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
/// 3. The production `ensure_interest` path installs the KP interest.
/// 4. The `IngestParser` (stand-in for `MarmotIngestParser`) must receive the
///    stored KP events WITHOUT any network activity.
///
/// Before this fix: step 4 produced 0 parser deliveries → MLS group creation
/// no-ops on relaunch.
#[test]
fn two_session_interest_install_kp_regression() {
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

    // ── Session 2: install the KP interest (production path) ─────────────────
    // `register_interest(Replace)` is the exact call the
    // `InterestsCommand::EnsureInterest` arm makes (Replace policy).
    let interest = kp_interest(999, &receiver_hex);
    {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        let identity = crate::subs::test_identity_for_interest(
            ("scoped-test-interest", interest.id.0),
            &interest,
        );
        kernel.register_interest(
            &[InterestRegistration {
                identity,
                interest,
                policy: InterestWrite::Replace,
            }],
            "replace-interest",
        );
    }
    drain_cache_serves(&mut kernel, 10);

    // ── Assert: parser received stored KP events, NO network needed ───────────
    let seen_s2 = parser_s2.seen_ids();
    assert!(
        seen_s2.contains(&kp_id_1),
        "TWO-SESSION REGRESSION FAIL: parser must receive KP event 1 ({kp_id_1}) \
         from the store on session-2 EnsureInterest; got {seen_s2:?}. \
         This is the MLS cross-session no-op bug."
    );
    assert!(
        seen_s2.contains(&kp_id_2),
        "TWO-SESSION REGRESSION FAIL: parser must receive KP event 2 ({kp_id_2}) \
         from the store on session-2 EnsureInterest; got {seen_s2:?}. \
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
fn interest_install_ingest_parser_idempotent_re_ingest() {
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
        let identity = crate::subs::test_identity_for_interest(
            ("scoped-test-interest", interest.id.0),
            &interest,
        );
        kernel.register_interest(
            &[InterestRegistration {
                identity,
                interest,
                policy: InterestWrite::Replace,
            }],
            "replace-interest",
        );
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

    // Second install via EnsureInterest with a new InterestId (fresh completion key).
    // This must NOT panic and must NOT deliver duplicate events (in-memory dedup).
    let interest_2 = author_kind1_interest(51, &author);
    {
        use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
        let identity = crate::subs::test_identity_for_interest(
            ("scoped-test-interest", interest_2.id.0),
            &interest_2,
        );
        kernel.register_interest(
            &[InterestRegistration {
                identity,
                interest: interest_2,
                policy: InterestWrite::Replace,
            }],
            "replace-interest",
        );
    }
    drain_cache_serves(&mut kernel, 10);

    let after_reserve = parser.seen_kinds();
    assert!(
        after_reserve.is_empty(),
        "idempotent re-serve: events already in the events cache must NOT be \
         re-dispatched; got {after_reserve:?}"
    );
}
