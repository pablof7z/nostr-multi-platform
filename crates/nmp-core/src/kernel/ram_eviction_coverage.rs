//! K3 Stage D3 (ADR-0072 §3.D3) — eviction⇄ledger coherence, kernel side.
//!
//! Two legs:
//!
//! - **Leg 1 — pin below the LEDGER floor** ([`Kernel::pin_floor_for_shape`]).
//!   The live since-floor for a covered shape comes from the coverage ledger's
//!   `covered_through`. The floor-coherent pin set
//!   (`super::Kernel::add_floor_coherent_pins`) must therefore pin events
//!   at/below the LEDGER floor so LRU eviction cannot strand a below-floor event
//!   the floored REQ will never re-fetch — using the SAME decision the
//!   `coverage_floor` read makes (single-source discipline: no second floor
//!   computation).
//!
//! - **Leg 2 — the eviction backstop guard set**
//!   ([`Kernel::derive_coverage_guards`]). Even if the pin set is bypassed or
//!   budget-truncated, the store must lower an over-claimed `covered_through`
//!   atomically with the below-floor delete. The kernel hands the store one
//!   [`CoverageGuard`](crate::store::CoverageGuard) per active covered
//!   `(filter_hash, relay)`, carrying the kernel-owned shape-match predicate so
//!   the store never links protocol/shape logic (D0). The store-layer
//!   atomicity is proven in `nmp-testing/tests/store_coverage_eviction_backstop`.
//!
//! Extracted from `ram_eviction.rs` (which sits at the 500-LOC hard cap).

use std::collections::HashSet;

use super::Kernel;
use crate::planner::{canonical_filter_hash, InterestShape};
use crate::store::{CoverageGuard, CoverageMatchFn};

// K3 Stage D3 kernel-layer tests (both legs). Declared here (not in
// `ram_eviction.rs`, which is at the LOC cap).
#[cfg(test)]
#[path = "gc_coverage_coherent_d3_tests.rs"]
mod gc_coverage_coherent_d3_tests;

impl Kernel {
    /// Extend `pins` with every stored event at or below each active floored
    /// shape's `since`-floor (#1090 Stage 2; K3 Stage D3 leg 1 — the floor source
    /// is the coverage ledger).
    ///
    /// For each active `LogicalInterest`, [`pin_floor_for_shape`] computes the
    /// floor a REQ for the shape will carry, then every stored event matching
    /// that shape with `created_at <= floor` is added to the pin set. Shapes
    /// with no floor (`None`) contribute nothing — they are un-floored, so the
    /// relay re-sends their history and no hole can form.
    ///
    /// Returns `true` when all shapes were fully scanned, `false` when any
    /// shape's scan was truncated by the `PIN_SCAN_MAX_EVENTS` budget. Callers
    /// must treat `false` conservatively — see `derive_store_pin_set`.
    pub(super) fn add_floor_coherent_pins(
        &self,
        pins: &mut HashSet<crate::store::EventId>,
    ) -> bool {
        use super::floor::{pin_shape_events_below_floor, PinScanOutcome};

        let active = self.lifecycle.registry().iter_active();
        if active.is_empty() {
            return true;
        }
        let mut complete = true;
        for interest in &active {
            // K3 Stage D3 leg 1: the pin floor is the floor a REQ for this shape
            // will carry — the coverage ledger, one decision shared with
            // `coverage_floor`.
            let Some(floor) = self.pin_floor_for_shape(&interest.shape) else {
                continue;
            };
            let outcome = pin_shape_events_below_floor(
                &interest.shape,
                floor,
                self.store.as_ref(),
                pins,
                super::floor::PIN_SCAN_MAX_EVENTS,
            );
            if outcome == PinScanOutcome::Truncated {
                tracing::warn!(
                    "floor-coherent pin scan truncated at {} events for shape \
                     (Etag/Ptag with many matches); LRU eviction deferred this tick. \
                     See #1348.",
                    super::floor::PIN_SCAN_MAX_EVENTS,
                );
                complete = false;
                // Do not break: keep scanning remaining shapes so we pin as many
                // events as possible within the overall budget. Each shape gets
                // its own fresh PIN_SCAN_MAX_EVENTS allowance.
            }
        }
        complete
    }

    /// K3 Stage D3 leg 1 — the floor the floor-coherent pin set must protect for
    /// `shape`, matching the floor a REQ for the shape will actually carry.
    ///
    /// This is the SAME decision the `coverage_floor` read makes, lifted to the
    /// relay-agnostic pin set:
    ///
    /// - **The ledger HAS ≥1 row** for `canonical_filter_hash(shape)` → the MAX
    ///   `covered_through` across that filter_hash's relays. The store is
    ///   relay-agnostic but the ledger is per-relay; a REQ on the relay with the
    ///   highest coverage floors highest, so pinning below the MAX protects every
    ///   event ANY covered REQ could floor away (over-pinning is always safe — it
    ///   only defers eviction; under-pinning punches the hole).
    /// - **NO row** → `None` — `coverage_floor` refuses the floor (un-floored full
    ///   `[0, ∞)` REQ), so the relay re-sends the whole history and no
    ///   floor-coherent pin is needed.
    ///
    /// There is no second floor computation: this reuses the
    /// `coverage_*_for_filter_hash` store primitives, mirroring `coverage_floor`.
    pub(super) fn pin_floor_for_shape(&self, shape: &InterestShape) -> Option<u64> {
        // The ledger is the floor authority. Max across the shape's relays;
        // no row ⇒ refuse the floor (no pin needed).
        let filter_hash = canonical_filter_hash(shape);
        self.store.coverage_max_for_filter_hash(&filter_hash)
    }

    /// K3 Stage D3 leg 2 — build one [`CoverageGuard`](crate::store::CoverageGuard)
    /// per active covered `(filter_hash, relay)` so the store's LRU eviction can
    /// lower `covered_through` atomically when it strands a below-floor event
    /// (the backstop that holds even if the pin set is bypassed or truncated).
    ///
    /// The `matches` predicate is the kernel-owned
    /// `InterestShape::matches_event_with_id` (D0: the shape-match predicate
    /// never leaks into the store), captured per shape.
    pub(crate) fn derive_coverage_guards(&self) -> Vec<CoverageGuard> {
        let active = self.lifecycle.registry().iter_active();
        let mut guards: Vec<CoverageGuard> = Vec::new();
        // De-dupe by (filter_hash, relay): distinct interests can canonicalise to
        // the same shape hash; one guard per ledger row is enough.
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for interest in &active {
            let filter_hash = canonical_filter_hash(&interest.shape);
            for (relay, covered_through) in self.store.coverage_rows_for_filter_hash(&filter_hash) {
                if !seen.insert((filter_hash.clone(), relay.clone())) {
                    continue;
                }
                let shape = interest.shape.clone();
                let matches: CoverageMatchFn = std::sync::Arc::new(
                    move |id: &str,
                          author: &str,
                          kind: u32,
                          created_at: u64,
                          tags: &[Vec<String>]| {
                        shape.matches_event_with_id(id, author, kind, created_at, tags)
                    },
                );
                guards.push(CoverageGuard {
                    filter_hash: filter_hash.clone(),
                    relay,
                    covered_through,
                    matches,
                });
            }
        }
        guards
    }
}
