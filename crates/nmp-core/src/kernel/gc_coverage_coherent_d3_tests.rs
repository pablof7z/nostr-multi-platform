//! K3 Stage D3 (ADR-0072 §3.D3) — eviction⇄ledger coherence, kernel layer.
//!
//! Two legs:
//!
//! 1. **Pin below the LEDGER floor.** The live since-floor for a covered shape
//!    comes from the coverage ledger's `covered_through`. So the floor-coherent
//!    pin set must pin events at/below the LEDGER floor — using the SAME
//!    `coverage_floor` source the floor read uses (single-source discipline; no
//!    second floor computation).
//!
//! 2. **Backstop guard set.** `derive_coverage_guards` builds a `CoverageGuard`
//!    per active covered `(filter_hash, relay)` for explicit finite durable
//!    retention, so the store can lower `covered_through` atomically if eviction
//!    strands a below-floor event. (The store-layer atomicity is proven in
//!    `nmp-testing/tests/store_coverage_eviction_backstop.rs`; here we prove the
//!    kernel hands the store the right guards.)
//!
//! No coverage row ⇒ no floor-coherent pin + empty guard set.
//!
//! RED-by-sabotage: see the module doc on each test for the line that must fail
//! if leg 1 reads a presence floor / leg 2 emits no guards.

use crate::kernel::ram_eviction_tests::{make_pubkey, pin_clock, T0_SECS};
use crate::kernel::Kernel;
use crate::planner::canonical_filter_hash;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use nmp_network::role::RelayRole;

fn open_interest(kernel: &mut Kernel, filter_json: &str, consumer_id: &str) {
    use crate::planner::{InterestLifecycle, InterestScope, LogicalInterest};
    use crate::subs::sub_key::{SubIdentity, SubKey, SubOwnerKey, SubScope};

    let shape = crate::planner::InterestShape::from_filter_json(filter_json)
        .expect("test filter must be a valid NIP-01 filter object");
    let key = SubKey::builder("open-interest")
        .with(&shape)
        .with(1u32)
        .finish();
    let identity = SubIdentity::new(SubOwnerKey::new(consumer_id), key, SubScope::Global);
    let interest = LogicalInterest {
        scope: InterestScope::Global,
        shape,
        lifecycle: InterestLifecycle::Tailing,
        ..LogicalInterest::default()
    };
    let _ = kernel.open_interest_sub(identity, interest);
}

fn inject_note(kernel: &mut Kernel, id: &str, pubkey: &str, created_at: u64) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: format!("note {id}"),
        sig: "a".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    kernel.ingest_pre_verified_event(RelayRole::Content, "", verified);
}

fn id_bytes(hex: &str) -> crate::store::EventId {
    let parsed = ::nostr::prelude::EventId::from_hex(hex).expect("valid hex id");
    let mut out = [0u8; 32];
    out.copy_from_slice(parsed.as_bytes());
    out
}

const RELAY: &str = "wss://relay.example/";

// ── Leg 1 — pin below the LEDGER floor when the flag is on ────────────────────

/// Flag ON + a coverage row whose `covered_through` is ABOVE the newest stored
/// event: every stored event at/below `covered_through` is pinned — because the
/// covered REQ will floor at `covered_through + 1` and never re-fetch them.
///
/// RED-by-sabotage: if leg 1 still reads the PRESENCE floor (newest stored =
/// 300), an event at 350 would NOT be below the presence floor and so would not
/// be pinned; the ledger floor (500) pins it. (Here all events are ≤300 so the
/// sharper check is that the LEDGER floor governs, see the companion test.)
#[test]
fn pins_below_ledger_floor_when_flag_on() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(9_101);
    let e_old = format!("{:0>64x}", 0xD30001u64);
    let e_mid = format!("{:0>64x}", 0xD30002u64);
    let e_new = format!("{:0>64x}", 0xD30003u64);
    inject_note(&mut kernel, &e_old, &author, 100);
    inject_note(&mut kernel, &e_mid, &author, 200);
    inject_note(&mut kernel, &e_new, &author, 300);

    let filter = format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#);
    open_interest(&mut kernel, &filter, "d3-pin-test");

    // Record coverage THROUGH 500 (above the newest stored event). The covered
    // REQ will floor at 501, so all three stored events (≤300) are below it.
    let shape = crate::planner::InterestShape::from_filter_json(&filter).unwrap();
    let fh = canonical_filter_hash(&shape);
    kernel.store.record_coverage(&fh, RELAY, 500);

    kernel.events.clear(); // store is the sole holder
    let (pins, _complete) = kernel.derive_store_pin_set();

    for (label, id) in [("old", &e_old), ("mid", &e_mid), ("new", &e_new)] {
        assert!(
            pins.contains(&id_bytes(id)),
            "{label} (≤300, below ledger covered_through=500) must be pinned"
        );
    }
}

/// The discriminating case: ledger floor LOWER than the presence floor. Flag ON
/// + a coverage row at 150 (below the newest stored event=300). The covered REQ
/// floors at 151, so ONLY events ≤150 are below the floor and must be pinned;
/// events at 200/300 (above the ledger floor) need no floor-coherent pin.
///
/// RED-by-sabotage: if leg 1 used the PRESENCE floor (300), it would pin the
/// 200 event too — over-pinning. The sharp assertion is that the 200 event is
/// NOT floor-pinned (it is above the ledger floor of 150). This proves the
/// LEDGER, not presence, is the pin floor source under the flag.
#[test]
fn ledger_floor_governs_not_presence_when_flag_on() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(9_102);
    let e_below = format!("{:0>64x}", 0xD30101u64); // 100 ≤ 150 → pinned
    let e_above = format!("{:0>64x}", 0xD30102u64); // 200 > 150 → NOT floor-pinned
    let e_top = format!("{:0>64x}", 0xD30103u64); // 300 > 150 → NOT floor-pinned
    inject_note(&mut kernel, &e_below, &author, 100);
    inject_note(&mut kernel, &e_above, &author, 200);
    inject_note(&mut kernel, &e_top, &author, 300);

    let filter = format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#);
    open_interest(&mut kernel, &filter, "d3-ledger-governs");
    let shape = crate::planner::InterestShape::from_filter_json(&filter).unwrap();
    let fh = canonical_filter_hash(&shape);
    kernel.store.record_coverage(&fh, RELAY, 150);

    kernel.events.clear();
    let (pins, _complete) = kernel.derive_store_pin_set();

    assert!(
        pins.contains(&id_bytes(&e_below)),
        "e_below (100 ≤ ledger floor 150) MUST be floor-pinned"
    );
    assert!(
        !pins.contains(&id_bytes(&e_above)),
        "e_above (200 > ledger floor 150) must NOT be floor-pinned — the LEDGER \
         floor (150), not the presence floor (300), governs under the flag"
    );
    assert!(
        !pins.contains(&id_bytes(&e_top)),
        "e_top (300 > ledger floor 150) must NOT be floor-pinned"
    );
}

/// Flag ON + NO coverage row ⇒ the covered REQ is un-floored (D2 refuses the
/// floor), so the relay re-sends the full history and NO floor-coherent pin is
/// needed. The pin set must NOT floor-pin the shape's events (over-retention).
///
/// RED-by-sabotage: if leg 1 fell back to the presence floor on no-row, it
/// would pin the below-presence-floor events; the correct behaviour (matching
/// D2's "no row ⇒ refuse the floor") is to pin nothing for the shape.
#[test]
fn no_row_under_flag_pins_nothing_for_the_shape() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(9_103);
    let e_old = format!("{:0>64x}", 0xD30201u64);
    let e_new = format!("{:0>64x}", 0xD30202u64);
    inject_note(&mut kernel, &e_old, &author, 100);
    inject_note(&mut kernel, &e_new, &author, 300);

    let filter = format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#);
    open_interest(&mut kernel, &filter, "d3-no-row");
    // No record_coverage → no row → D2 refuses the floor → no pin needed.

    kernel.events.clear();
    let (pins, _complete) = kernel.derive_store_pin_set();

    assert!(
        !pins.contains(&id_bytes(&e_old)),
        "no coverage row ⇒ un-floored REQ ⇒ no floor-coherent pin for e_old"
    );
    assert!(
        !pins.contains(&id_bytes(&e_new)),
        "no coverage row ⇒ un-floored REQ ⇒ no floor-coherent pin for e_new"
    );
}

// ── Leg 2 — the kernel hands the store a guard per covered (filter_hash, relay)

/// Flag ON + a coverage row ⇒ `derive_coverage_guards` yields one guard for the
/// covered `(filter_hash, relay)` whose `matches` predicate accepts the shape's
/// events and rejects others, carrying the recorded `covered_through`.
///
/// RED-by-sabotage: if leg 2 emits no guards (or the wrong floor/match), these
/// asserts fail — the store would have no way to lower the row on eviction.
#[test]
fn derive_coverage_guards_emits_guard_for_covered_shape() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(9_201);
    let filter = format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#);
    open_interest(&mut kernel, &filter, "d3-guards");
    let shape = crate::planner::InterestShape::from_filter_json(&filter).unwrap();
    let fh = canonical_filter_hash(&shape);
    kernel.store.record_coverage(&fh, RELAY, 400);

    let guards = kernel.derive_coverage_guards();
    let g = guards
        .iter()
        .find(|g| g.filter_hash == fh && g.relay == RELAY)
        .expect("a guard for the covered (filter_hash, relay) must exist");
    assert_eq!(g.covered_through, 400);
    // The guard matches the shape's events (Alice/kind1) and rejects a kind:7.
    // `matches` wraps the pure kernel predicate
    // `InterestShape::matches_event_with_id` (NOT a host/FFI closure); these are
    // direct unit assertions on that predicate, never a kernel-thread invocation.
    assert!((g.matches)("anyid", &author, 1, 200, &[])); // doctrine-allow: D15 — pure kernel predicate, test assertion
    assert!(!(g.matches)("anyid", &author, 7, 200, &[])); // doctrine-allow: D15 — pure kernel predicate, test assertion
}

/// No active interest with a coverage row ⇒ no guards (nothing to back-stop).
#[test]
fn derive_coverage_guards_is_empty_without_covered_interest() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(9_202);
    let filter = format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#);
    // Interest open but NO coverage row recorded → no guard to emit.
    open_interest(&mut kernel, &filter, "d3-guards-none");

    assert!(
        kernel.derive_coverage_guards().is_empty(),
        "no coverage row for the active interest ⇒ no coverage guards"
    );
}

// ── Integrated — real `run_gc_step` default durable-retention policy ───────────

/// The integrated oracle through the REAL production `run_gc_step` (the sole
/// production GC caller) after #1480. With the flag on and an active covered
/// follow-feed interest:
///
/// - production durable LRU deletion is disabled, so valid fetched events remain
///   queryable even beyond the historical 10k ceiling;
/// - the coverage ledger stays unchanged because no durable below-floor row was
///   deleted; and
/// - the explicit finite-retention backstop remains covered by
///   `store_coverage_eviction_backstop.rs`.
///
/// Inserts just over the historical durable ceiling to prove it no longer
/// controls production row retention.
#[test]
fn run_gc_step_default_retains_durable_events_and_ledger() {
    use crate::store::DEFAULT_DURABLE_EVENT_CEILING;

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(9_301);
    let filter = format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#);
    let shape = crate::planner::InterestShape::from_filter_json(&filter).unwrap();
    let fh = canonical_filter_hash(&shape);

    // The oldest covered event (t=10) — the prime LRU victim absent a pin.
    let oldest = format!("{:0>64x}", 0xD3F001u64);
    inject_note(&mut kernel, &oldest, &author, 10);
    // Fill past the historical durable ceiling with NEWER unrelated events
    // (different author, so they neither match the guard nor the shape's
    // floor-coherent pin).
    let filler_author = make_pubkey(9_302);
    let first_filler = format!("{:0>64x}", 0xE00000u64);
    for i in 0..(DEFAULT_DURABLE_EVENT_CEILING as u64 + 50) {
        let id = format!("{:0>64x}", 0xE00000u64 + i);
        inject_note(&mut kernel, &id, &filler_author, 1_000 + i);
    }

    // Coverage claims [0, 100] for the shape — above the oldest (10), so the
    // leg-1 ledger-floor pin must protect every covered event ≤100 (the t=10).
    kernel.store.record_coverage(&fh, RELAY, 100);

    // An active follow-feed interest so leg 1 derives the ledger pin.
    open_interest(&mut kernel, &filter, "d3-integrated");

    let report = kernel.run_gc_step().expect("gc pass ran");
    assert_eq!(
        report.lru_evicted, 0,
        "production gc must not LRU-delete valid durable rows by default (#1480)"
    );

    // The t=10 covered event survives. Under #1480 this is true because
    // production durable LRU is disabled; the pin/backstop path is still tested
    // by the unit tests above and the store-layer explicit-retention tests.
    assert!(
        kernel
            .store
            .get_by_id(&id_bytes(&oldest))
            .unwrap()
            .is_some(),
        "the below-covered_through event must remain durable after production GC"
    );
    assert!(
        kernel
            .store
            .get_by_id(&id_bytes(&first_filler))
            .unwrap()
            .is_some(),
        "over-historical-ceiling filler event must remain durable after production GC"
    );
    // Coherence: no durable row was lost, so the ledger is unchanged.
    assert_eq!(
        kernel.store.get_coverage(&fh, RELAY),
        Some(100),
        "no below-floor covered event was deleted ⇒ the ledger stays honest at 100"
    );
}
