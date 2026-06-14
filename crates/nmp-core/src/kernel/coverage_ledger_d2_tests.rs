//! K3 Stage D2 (ADR-0056 §3.D2) — coverage-ledger READ-swap tests.
//!
//! D1 (`coverage_ledger_d1_tests.rs`) proved the WRITE path; these tests prove
//! the READ swap: the since-floor reads the coverage ledger for
//! `(canonical_filter_hash(shape), relay)` when the flag is ON, with a
//! presence-derived fallback ONLY when the ledger has no row, and behaviour
//! identical to today when the flag is OFF.
//!
//! Two layers, both load-bearing:
//!
//! 1. **The decision table** (`Kernel::coverage_floor_for`) — flag off ⇒
//!    presence; flag on + row ⇒ ledger `covered_through`; flag on + no row ⇒
//!    presence. Driven directly so the table is pinned without standing up the
//!    full recompile path.
//! 2. **The production `WatermarkFn` closure** (via the relay-aware
//!    `watermark_for_shape_on_relay_for_test` accessor) — proves the kernel's
//!    INSTALLED closure reads the ledger by the canonical hash AND threads the
//!    relay through, so the SAME shape on two relays can floor differently.
//!
//! The fixture-relay journey test (the merge gate) lives in
//! `crates/nmp-testing/tests/` and exercises the real recompile→REQ→ingest path
//! end to end; these unit tests are necessary but, per ADR-0056 §4, not
//! sufficient on their own.

use std::collections::BTreeSet;

use crate::kernel::Kernel;
use crate::planner::{canonical_filter_hash, InterestShape};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};

const RELAY_A: &str = "wss://relay-a.coverage-d2";
const RELAY_B: &str = "wss://relay-b.coverage-d2";

fn hex_pk(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

/// An author+kind follow-feed shape (the H1 shape: kind:1 by one author).
fn author_kind_shape(author_hex: &str) -> InterestShape {
    InterestShape {
        authors: BTreeSet::from([author_hex.to_string()]),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    }
}

/// Insert one kind:1 event by `author_hex` into the kernel store, so the
/// PRESENCE watermark for an author+kind shape is `created_at`.
fn insert_author_event(kernel: &mut Kernel, id_hex: &str, author_hex: &str, created_at: u64) {
    let raw = RawEvent {
        id: id_hex.to_string(),
        pubkey: author_hex.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };
    kernel
        .event_store_handle()
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://ingest.example".to_string(),
            created_at.saturating_mul(1_000),
        )
        .expect("author event insert");
}

/// Write a coverage row directly onto the kernel store for `(shape, relay)`.
/// Mirrors what the D1 write path records at EOSE/NEG-DONE, but lets a D2 test
/// seed coverage deterministically. Keyed by the SAME `canonical_filter_hash`
/// the recompile floor reads by.
fn seed_coverage(kernel: &Kernel, shape: &InterestShape, relay: &str, covered_through: u64) {
    let filter_hash = canonical_filter_hash(shape);
    kernel
        .event_store_handle()
        .record_coverage(&filter_hash, relay, covered_through);
}

// ─── Layer 1: the decision table (`coverage_floor_for`) ─────────────────────────

/// Flag OFF: the relay and ledger are ignored; the presence closure result is
/// returned verbatim. This is the default, byte-identical-to-today path.
#[test]
fn floor_is_presence_when_flag_off_even_if_ledger_has_a_row() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // The ledger defaults ON now; pin the flag-OFF path explicitly.
    kernel.set_coverage_ledger_enabled(false);
    let shape = author_kind_shape(&hex_pk("aa"));
    // A ledger row exists, but the flag is OFF — it must be ignored entirely.
    seed_coverage(&kernel, &shape, RELAY_A, 1_600_000_000);
    assert!(!kernel.coverage_ledger_enabled());

    let floor = kernel.coverage_floor_for(&shape, RELAY_A, || Some(1_700_000_000));
    assert_eq!(
        floor,
        Some(1_700_000_000),
        "flag OFF must return the presence floor verbatim, ignoring the ledger row",
    );

    // And the presence closure's None is preserved too (no ledger leak).
    let floor_none = kernel.coverage_floor_for(&shape, RELAY_A, || None);
    assert_eq!(
        floor_none, None,
        "flag OFF must return the presence None verbatim, ignoring the ledger row",
    );
}

/// Flag ON + ledger HAS a row: the floor is the ledger's `covered_through`, NOT
/// the presence value. The presence closure must not even be consulted.
#[test]
fn floor_reads_ledger_covered_through_when_flag_on_and_row_present() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_coverage_ledger_enabled(true);
    let shape = author_kind_shape(&hex_pk("aa"));
    seed_coverage(&kernel, &shape, RELAY_A, 1_650_000_000);

    let presence_consulted = std::cell::Cell::new(false);
    let floor = kernel.coverage_floor_for(&shape, RELAY_A, || {
        presence_consulted.set(true);
        Some(1_700_000_000)
    });

    assert_eq!(
        floor,
        Some(1_650_000_000),
        "flag ON + row present must floor at the ledger's covered_through, not presence",
    );
    assert!(
        !presence_consulted.get(),
        "a ledger HIT must NOT consult the (potentially expensive) presence closure",
    );
}

/// Flag ON + ledger has NO row: REFUSE to floor (full window). The H1 fix —
/// an un-synced `(filter_hash, relay)` must NOT inherit the (unsound) presence
/// floor, else a stray below-floor event would suppress backfill. The presence
/// closure must not even be consulted on the flag-on path.
#[test]
fn floor_is_refused_when_flag_on_but_no_row() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_coverage_ledger_enabled(true);
    let shape = author_kind_shape(&hex_pk("aa"));
    // No coverage row seeded for this (shape, relay).

    let presence_consulted = std::cell::Cell::new(false);
    let floor = kernel.coverage_floor_for(&shape, RELAY_A, || {
        presence_consulted.set(true);
        Some(1_700_000_000)
    });
    assert_eq!(
        floor, None,
        "flag ON + NO row must REFUSE the floor (full window), not re-inherit \
         the unsound presence floor (the H1 fix)",
    );
    assert!(
        !presence_consulted.get(),
        "under flag ON the presence heuristic must NEVER govern the floor — \
         coverage is the sole authority",
    );
}

/// Flag ON: coverage is per-`(filter_hash, relay)`. The SAME shape with a row
/// on relay A but none on relay B floors from the ledger on A and from presence
/// on B — the relay-threading correctness at the decision-table layer.
#[test]
fn floor_is_per_relay_when_flag_on() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_coverage_ledger_enabled(true);
    let shape = author_kind_shape(&hex_pk("aa"));
    // Relay A has completed coverage through 1_650_000_000; relay B has none.
    seed_coverage(&kernel, &shape, RELAY_A, 1_650_000_000);

    let floor_a = kernel.coverage_floor_for(&shape, RELAY_A, || Some(1_700_000_000));
    let floor_b = kernel.coverage_floor_for(&shape, RELAY_B, || Some(1_700_000_000));

    assert_eq!(
        floor_a,
        Some(1_650_000_000),
        "relay A has a coverage row → ledger floor at covered_through",
    );
    assert_eq!(
        floor_b, None,
        "relay B has NO coverage row → floor refused / full window \
         (different floor, same shape — the per-relay threading)",
    );
}

// ─── Layer 2: the installed production `WatermarkFn` closure ─────────────────────

/// The kernel's INSTALLED watermark closure (not a test stub) reads the ledger
/// by the canonical filter hash when the flag is ON and a row is present —
/// proving the `mod.rs` closure wiring, not just the `coverage_floor_for`
/// method. The presence watermark (newest stored event) is DIFFERENT from the
/// ledger value, so the assertion distinguishes the two sources.
#[test]
fn installed_closure_reads_ledger_when_flag_on() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let author = hex_pk("aa");
    let shape = author_kind_shape(&author);
    // Presence: newest stored kind:1 by this author is at 1_700_000_000.
    insert_author_event(&mut kernel, &hex_pk("e0"), &author, 1_700_000_000);
    // Ledger: completed coverage only through 1_650_000_000 on RELAY_A.
    kernel.set_coverage_ledger_enabled(true);
    seed_coverage(&kernel, &shape, RELAY_A, 1_650_000_000);

    let floor = kernel
        .lifecycle
        .watermark_for_shape_on_relay_for_test(&shape, RELAY_A);

    assert_eq!(
        floor,
        Some(1_650_000_000),
        "the installed closure must floor at the ledger covered_through (1_650…), \
         not the presence watermark (1_700…)",
    );
}

/// Flag OFF (default): the installed closure floors at the PRESENCE watermark
/// (newest stored event), ignoring any ledger row — the regression guard that
/// proves D2 is dormant by default.
#[test]
fn installed_closure_uses_presence_when_flag_off() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // The ledger defaults ON now; pin the flag-OFF path explicitly.
    kernel.set_coverage_ledger_enabled(false);
    let author = hex_pk("aa");
    let shape = author_kind_shape(&author);
    insert_author_event(&mut kernel, &hex_pk("e0"), &author, 1_700_000_000);
    // A ledger row exists but the flag is OFF — it must be ignored.
    seed_coverage(&kernel, &shape, RELAY_A, 1_650_000_000);
    assert!(!kernel.coverage_ledger_enabled());

    let floor = kernel
        .lifecycle
        .watermark_for_shape_on_relay_for_test(&shape, RELAY_A);

    assert_eq!(
        floor,
        Some(1_700_000_000),
        "flag OFF: the installed closure must floor at the presence watermark \
         (1_700…), proving the read swap is dormant by default",
    );
}

/// The H1 headline at the closure layer — the load-bearing invariant of D2.
///
/// Author A's thread reply (a kind:1 by A, acquired under an Etag/thread shape)
/// is stored at t=1_700_000_000 — a STRAY event. Then we follow A; the
/// follow-feed shape is `authors:[A], kinds:[1]`, which has NO coverage row of
/// its own (the stray was never fetched under THIS shape's REQ). Under the OLD
/// presence floor this shape would floor at the stray (1_700…), suppressing A's
/// history below it. Under D2 the floor is REFUSED (no coverage row ⇒ full
/// window), so A's full history backfills. This is the unit-level mirror of the
/// fixture-relay merge-gate journey test.
#[test]
fn installed_closure_refuses_floor_for_followfeed_with_stray_but_no_coverage() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let author = hex_pk("bb");
    let shape = author_kind_shape(&author);
    // The stray thread-reply by A is on disk (presence WOULD floor at 1_700…).
    insert_author_event(&mut kernel, &hex_pk("57"), &author, 1_700_000_000);
    kernel.set_coverage_ledger_enabled(true);
    // No coverage row for the follow-feed (filter_hash, RELAY_A): the stray was
    // acquired under a different shape, so the follow feed is un-synced here.

    let floor = kernel
        .lifecycle
        .watermark_for_shape_on_relay_for_test(&shape, RELAY_A);
    assert_eq!(
        floor, None,
        "H1: a stray stored event must NOT floor the un-synced follow feed — \
         no coverage row ⇒ refuse the floor ⇒ full backfill (presence would \
         have floored at 1_700… and suppressed history)",
    );
}

/// Per-relay isolation at the closure layer: a coverage row on relay B must not
/// leak into relay A's floor. Same shape, A uncovered (refused/full), B covered
/// (floored at its `covered_through`).
#[test]
fn installed_closure_coverage_does_not_leak_across_relays() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_coverage_ledger_enabled(true);
    let author = hex_pk("cc");
    let shape = author_kind_shape(&author);
    seed_coverage(&kernel, &shape, RELAY_B, 1_650_000_000);

    let floor_a = kernel
        .lifecycle
        .watermark_for_shape_on_relay_for_test(&shape, RELAY_A);
    let floor_b = kernel
        .lifecycle
        .watermark_for_shape_on_relay_for_test(&shape, RELAY_B);
    assert_eq!(floor_a, None, "RELAY_A has no row ⇒ floor refused");
    assert_eq!(
        floor_b,
        Some(1_650_000_000),
        "RELAY_B has a row ⇒ ledger floor; coverage must not leak from B to A",
    );
}
