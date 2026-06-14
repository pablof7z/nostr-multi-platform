//! #1090 Stage 2 — floor-coherent store-tier eviction pins.
//!
//! ## The hole this closes
//!
//! The live `since`-floor for a subscription is content-derived: the
//! `watermark_fn` (`kernel/mod.rs`) queries the store for the newest stored
//! event matching each REQ shape and floors that REQ's `since` to `floor + 1`,
//! so the relay does not re-emit events already on disk.
//!
//! LRU eviction (Stage 3 ceiling) is free to delete a *middle* event that is
//! older than the surviving newest event. The floor stays at `newest + 1`, so
//! the self-healing REQ — which only asks for events newer than the floor —
//! will NEVER re-request that middle event: a permanent hole.
//!
//! The fix: `Kernel::derive_store_pin_set` additionally pins every stored event
//! at or below each active floored shape's `since`-floor, so the middle event
//! is never an eviction candidate while the shape's floor sits above it.
//!
//! These tests register a real active `LogicalInterest` (via the same
//! generic `open_interest_sub` seam the actor uses) and ingest events through
//! the real pre-verified ingest path, then assert the derived store pin set.

use super::super::ram_eviction_tests::{make_pubkey, pin_clock, T0_SECS};
use super::super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::{RawEvent, VerifiedEvent};

/// Register a generic `open_interest` on the kernel from a verbatim NIP-01
/// filter — mirrors the `ActorCommand::OpenInterest` dispatch arm body. Copied
/// from `ram_eviction_view_pin_tests` because the dispatch helper is private to
/// the actor module and these tests exercise the kernel pin invariant directly.
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

/// Ingest one kind:1 note through the real pre-verified ingest path so it lands
/// in BOTH the RAM `events` map and the authoritative `self.store`.
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

/// Ingest one kind:1 note tagged with an `e` reference (for Etag queries).
fn inject_note_with_etag(
    kernel: &mut Kernel,
    id: &str,
    pubkey: &str,
    created_at: u64,
    etag_target: &str,
) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 1,
        tags: vec![vec!["e".to_string(), etag_target.to_string()]],
        content: format!("note {id}"),
        sig: "a".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    kernel.ingest_pre_verified_event(RelayRole::Content, "", verified);
}

/// Convert a 64-hex id string into a store `EventId` ([u8; 32]).
fn id_bytes(hex: &str) -> crate::store::EventId {
    let parsed = ::nostr::prelude::EventId::from_hex(hex).expect("valid hex id");
    let mut out = [0u8; 32];
    out.copy_from_slice(parsed.as_bytes());
    out
}

/// The core Stage-2 invariant: for an active author+kind interest whose floor
/// sits at the newest stored event, EVERY older stored event matching the shape
/// (the "middle" and "old" events below the floor) is in the derived store pin
/// set — so LRU eviction can never punch a hole the floored REQ won't re-fetch.
#[test]
fn derive_store_pin_set_pins_events_below_shape_floor() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(7_001);
    let e_old = format!("{:0>64x}", 0xF00001u64);
    let e_mid = format!("{:0>64x}", 0xF00002u64);
    let e_new = format!("{:0>64x}", 0xF00003u64);

    inject_note(&mut kernel, &e_old, &author, 100);
    inject_note(&mut kernel, &e_mid, &author, 200);
    inject_note(&mut kernel, &e_new, &author, 300);

    // Register an active floored author+kind interest (kind:1). The store's
    // newest matching event (created_at=300) sets this shape's `since`-floor.
    open_interest(
        &mut kernel,
        &format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#),
        "floor-coherent-test",
    );

    // Simulate prior RAM-tier eviction: drop the events from the RAM `events`
    // map so the STORE is their sole holder. This is the exact hole scenario —
    // `open_view_pins` scans only RAM, so without the Stage-2 store-scan the
    // below-floor events would NOT be pinned and store LRU could evict them
    // permanently (the floored REQ asks only for created_at > 300).
    kernel.events.clear();

    let (pins, _complete) = kernel.derive_store_pin_set();

    // The newest event would survive LRU on its own merit; the hole risk is
    // the OLD and MID events, both below the floor (300). Stage 2 must pin them
    // from the store scan even though they are absent from the RAM map.
    assert!(
        pins.contains(&id_bytes(&e_old)),
        "e_old (created_at=100, below floor=300) must be pinned from the store scan"
    );
    assert!(
        pins.contains(&id_bytes(&e_mid)),
        "e_mid (created_at=200, below floor=300) must be pinned from the store scan"
    );
}

// ── #1348 — D8 scan budget for Etag/Ptag ─────────────────────────────────────

/// #1348 — `derive_store_pin_set` returns `complete = true` when the store has
/// few events well within `PIN_SCAN_MAX_EVENTS`. Common-path regression guard:
/// the bool must be `true` for small stores so the caller does not
/// unnecessarily skip LRU eviction.
#[test]
fn derive_store_pin_set_returns_complete_for_small_stores() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(8_001);
    let e_old = format!("{:0>64x}", 0xE10001u64);
    let e_new = format!("{:0>64x}", 0xE10002u64);
    inject_note(&mut kernel, &e_old, &author, 100);
    inject_note(&mut kernel, &e_new, &author, 300);

    open_interest(
        &mut kernel,
        &format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#),
        "complete-flag-test",
    );

    kernel.events.clear();
    let (pins, complete) = kernel.derive_store_pin_set();

    assert!(
        complete,
        "derive_store_pin_set must return complete=true for a small store \
         well within PIN_SCAN_MAX_EVENTS"
    );
    assert!(
        pins.contains(&id_bytes(&e_old)),
        "e_old must be pinned in the complete case"
    );
}

/// #1348 — when `pin_shape_events_below_floor` exhausts the `max_events` budget
/// before visiting all Etag-matching events, it returns `PinScanOutcome::Truncated`.
/// The pinned count must not exceed the budget: events beyond the cap are
/// unvisited and must NOT be in the pin set (safety: the caller defers eviction).
#[test]
fn pin_scan_truncated_returns_truncated_outcome_and_stays_within_budget() {
    use super::floor::{pin_shape_events_below_floor, shape_floor, PinScanOutcome};
    use crate::planner::InterestShape;
    use std::collections::HashSet;

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    // A 64-hex target id (not a pubkey — Etag scans accept any 32-byte value
    // as the target). Use a deterministic value.
    let etag_target = format!("{:0>64x}", 0xABCD1234u64);
    let pubkey = make_pubkey(8_002);

    // Ingest 3 events tagged with the same `e` target.
    // budget = 2, so 3 events guarantees truncation.
    let budget = 2usize;
    let n = budget + 1;
    let ids: Vec<String> = (0..n as u64)
        .map(|i| format!("{:0>64x}", 0xDD0010u64 + i))
        .collect();
    for (i, id) in ids.iter().enumerate() {
        inject_note_with_etag(&mut kernel, id, &pubkey, 100 + i as u64 * 100, &etag_target);
    }

    // Build an Etag-shape filter: {"kinds":[1],"#e":["<target>"]}
    // Use r##"..."## so the `"#` in `"#e"` does not terminate the raw string.
    let filter_json = format!(r##"{{"kinds":[1],"#e":["{}"]}}"##, etag_target);
    let shape = InterestShape::from_filter_json(&filter_json).expect("valid Etag filter");

    let floor = shape_floor(&shape, kernel.store.as_ref(), &HashSet::new())
        .expect("floor must exist: events are stored");

    let mut pins: HashSet<crate::store::EventId> = HashSet::new();
    let outcome =
        pin_shape_events_below_floor(&shape, floor, kernel.store.as_ref(), &mut pins, budget);

    assert_eq!(
        outcome,
        PinScanOutcome::Truncated,
        "scan with budget={budget} against {n} events must be Truncated"
    );
    assert!(
        pins.len() <= budget,
        "truncated scan must pin at most budget={budget} events, got {}",
        pins.len()
    );
}

/// #1348 — coherence: when pin scan is truncated, `derive_store_pin_set`
/// returns `complete = false`, and `run_gc_step` must not evict any store
/// events that tick (LRU eviction deferred).
///
/// We verify the coherence guarantee indirectly: after running a GC step
/// whose pin scan is forcibly considered incomplete (by clearing events so
/// the scan has nothing to do but having no interest set up → scan is vacuously
/// complete), we confirm no event that was in the store is lost.
///
/// NOTE: we cannot force truncation through the public `run_gc_step` interface
/// with a MemEventStore holding fewer events than `PIN_SCAN_MAX_EVENTS`, so we
/// test the `complete = false` path through `derive_store_pin_set`'s API shape:
/// the prior test exercises `pin_shape_events_below_floor` truncation directly;
/// this test verifies that when `run_gc_step` has `complete = true` (small
/// store), it still correctly executes the full pass and records a GC report.
#[test]
fn run_gc_step_succeeds_and_records_report_after_pin_scan() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(8_003);
    let e_old = format!("{:0>64x}", 0xF10001u64);
    let e_new = format!("{:0>64x}", 0xF10002u64);
    inject_note(&mut kernel, &e_old, &author, 100);
    inject_note(&mut kernel, &e_new, &author, 300);

    open_interest(
        &mut kernel,
        &format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#),
        "gc-step-budget-test",
    );

    // run_gc_step must succeed (return Some) and record a report.
    let report = kernel
        .run_gc_step()
        .expect("run_gc_step must succeed against the in-memory store");

    // No events expired — lru_evicted may be 0 since store is small.
    // The critical invariant: the call completed and the report is populated.
    assert!(
        report.duration_ms < 10_000,
        "gc step must complete well within 10 s, got {}ms",
        report.duration_ms
    );

    // Below-floor event must still be in the store — not evicted.
    let e_old_bytes = {
        let parsed = ::nostr::prelude::EventId::from_hex(&e_old).unwrap();
        let mut out = [0u8; 32];
        out.copy_from_slice(parsed.as_bytes());
        out
    };
    assert!(
        kernel
            .store
            .get_by_id(&e_old_bytes)
            .expect("store lookup must not error")
            .is_some(),
        "e_old (below floor) must not be evicted when the pin scan is complete"
    );
}

// ── #1380 K3 Bug 2 — `since:None` pin-scan normalization ─────────────────────

/// #1380 Bug 2 — `pin_shape_events_below_floor` must clear `shape.since` before
/// applying the `<= floor` bound, EXACTLY mirroring `shape_floor`'s probe
/// normalization.
///
/// The bug: `shape_to_store_queries(shape)` embeds `shape.since`. A shape with
/// `shape.since = Some(T)` where `T > floor` makes the pin scan run an INVERTED
/// range `{ since: Some(T), until: Some(floor) }` → the store returns ZERO
/// events → the scan vacuously reports `Complete` → the below-floor event is
/// NOT pinned → LRU eviction is free to drop it → a permanent floor-coherence
/// hole (silent missing thread replies / DMs). Its sibling `shape_floor`
/// already normalizes `since = None` before probing; the pin scan must too.
///
/// This is the only test in the floor/eviction suite that exercises a non-`None`
/// `shape.since` — the gap the K3 verifier flagged. The shape is `author+kind`
/// (an `AuthorKind` query carries `since`, unlike `Etag`/`Ptag`), with
/// `shape.since` set NEWER than the stored event's `created_at`, so the inverted
/// range is provably empty pre-fix.
///
/// FAILS pre-fix (pin set empty → assertion that the below-floor event is pinned
/// fails); passes once the pin scan clears `since` to `None`.
#[test]
fn pin_shape_events_below_floor_clears_since_for_author_kind_shape() {
    use super::floor::{pin_shape_events_below_floor, shape_floor, PinScanOutcome};
    use crate::planner::InterestShape;
    use std::collections::HashSet;

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(7_701);
    let e_old = format!("{:0>64x}", 0xB20001u64);
    let e_new = format!("{:0>64x}", 0xB20002u64);

    // Two stored notes: old (100) and new (300). The floor sits at the newest
    // (300); the old event lives below the floor and must be pinned.
    inject_note(&mut kernel, &e_old, &author, 100);
    inject_note(&mut kernel, &e_new, &author, 300);

    // A floored author+kind shape carrying `since = Some(250)` — NEWER than the
    // below-floor event (100) AND newer than the floor target (300 is the floor,
    // but the inverted range `{since:250, until:300}` still excludes the
    // created_at=100 event regardless). Pre-fix the pin scan runs
    // `{since:Some(250), until:Some(300)}` and the e_old (100) event falls
    // outside the window → not pinned. With the bug it would even run the truly
    // inverted `{since:Some(350), until:floor}` form for a since past the floor;
    // we use 250 (below floor) so the window is non-empty for the NEW event yet
    // still drops e_old — proving `since` is what excludes the below-floor tail.
    let shape = InterestShape {
        authors: std::collections::BTreeSet::from([author.clone()]),
        kinds: std::collections::BTreeSet::from([1u32]),
        since: Some(250),
        ..Default::default()
    };

    // The floor itself is `since`-blind (shape_floor normalizes since=None), so
    // it correctly resolves to the newest stored match (300).
    let floor = shape_floor(&shape, kernel.store.as_ref(), &HashSet::new())
        .expect("author+kind shape with stored events must have a floor");
    assert_eq!(floor, 300, "floor must be the newest stored created_at");

    let mut pins: HashSet<crate::store::EventId> = HashSet::new();
    let outcome = pin_shape_events_below_floor(
        &shape,
        floor,
        kernel.store.as_ref(),
        &mut pins,
        super::floor::PIN_SCAN_MAX_EVENTS,
    );

    assert_eq!(
        outcome,
        PinScanOutcome::Complete,
        "scan must complete within budget"
    );
    assert!(
        pins.contains(&id_bytes(&e_old)),
        "#1380 Bug 2: the below-floor event (created_at=100) MUST be pinned even \
         though shape.since=Some(250) — the pin scan must clear `since` to None \
         before applying the `<= floor` bound, mirroring shape_floor. Pre-fix the \
         embedded since produces an inverted/exclusionary range that drops it, \
         leaving it open to LRU eviction (permanent floor-coherence hole)."
    );
}

// ── Original tests (unchanged signature, updated for derive_store_pin_set return type) ─

/// A stored event for an author with NO active interest must NOT be pinned by
/// the floor-coherent extension (it has no floored shape to protect it).
#[test]
fn derive_store_pin_set_does_not_pin_events_with_no_active_interest() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    // Author A: floored interest active.
    let author_a = make_pubkey(7_101);
    let a_old = format!("{:0>64x}", 0xE00001u64);
    let a_new = format!("{:0>64x}", 0xE00002u64);
    inject_note(&mut kernel, &a_old, &author_a, 100);
    inject_note(&mut kernel, &a_new, &author_a, 300);
    open_interest(
        &mut kernel,
        &format!(r#"{{"kinds":[1],"authors":["{author_a}"]}}"#),
        "floor-coherent-test",
    );

    // Author B: NO active interest. Its cold event must not be pinned.
    let author_b = make_pubkey(7_202);
    let b_cold = format!("{:0>64x}", 0xE00099u64);
    inject_note(&mut kernel, &b_cold, &author_b, 150);

    // Drop RAM holders so only the store + the floor-coherent scan can pin.
    kernel.events.clear();

    let (pins, _complete) = kernel.derive_store_pin_set();

    assert!(
        pins.contains(&id_bytes(&a_old)),
        "author A's below-floor event must be pinned"
    );
    assert!(
        !pins.contains(&id_bytes(&b_cold)),
        "author B has no active interest — its event must NOT be pinned"
    );
}
