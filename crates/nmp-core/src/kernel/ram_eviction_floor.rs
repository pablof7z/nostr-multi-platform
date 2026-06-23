//! #1090 Stage 2 — floor-coherent store-scan helpers for
//! [`Kernel::derive_store_pin_set`](super::super::Kernel::derive_store_pin_set).
//!
//! ## The hole these helpers close
//!
//! The live `since`-floor for a subscription comes from the coverage ledger: the
//! kernel's `watermark_fn` (`kernel/mod.rs`) floors each REQ's `since` to the
//! relay's `covered_through` + 1, so the relay does not re-emit events already
//! covered. An explicit finite durable-retention policy can delete a *middle*
//! event below `covered_through` — the floor stays at `covered_through + 1`, so
//! the self-healing REQ never re-requests the evicted middle event: a permanent
//! hole (unless eviction lowers the ledger; see Stage D3 leg 2).
//!
//! [`pin_shape_events_below_floor`] enumerates every stored event matching the
//! shape at or below the floor a REQ will carry (the coverage-ledger floor; see
//! [`Kernel::pin_floor_for_shape`](super::Kernel::pin_floor_for_shape)) so
//! `derive_store_pin_set` can pin them.
//!
//! ## K3 Stage C — single shape→query mapping (ADR-0056 §3)
//!
//! [`pin_shape_events_below_floor`] reads the SAME [`compile_store_query_plan`]
//! mapping cache-serve reads (iterating the mapping and applying the `<= floor`
//! bound). There is one shape→`StoreQuery` mapping, not a hand-synced copy, so
//! the pin scan can never miss a shape (or timestamp) the serve mapping covers.
//! (See `gc_floor_coherent_tests` for the floor⇄serve pin guards.)
//!
//! ## D8 scan budget (#1348)
//!
//! `Etag`/`Ptag` store queries have no `until` index bound (the secondary index
//! is keyed by target only, not by timestamp), so the floor is enforced only in
//! the visitor. Without a per-visitor count cap this scan is unbounded and
//! violates D8 (bounded work per tick). [`pin_shape_events_below_floor`]
//! therefore accepts a `max_events` count budget (see `PIN_SCAN_MAX_EVENTS`)
//! and returns a [`PinScanOutcome`] indicating whether the scan completed or was
//! truncated.
//!
//! **Safety on truncation**: pinning is a SAFETY mechanism — truncating a scan
//! means we cannot guarantee we have pinned every below-floor event for that
//! shape. The caller (`Kernel::add_floor_coherent_pins`) treats truncation
//! conservatively: it returns `false`, and `run_gc_step` skips the LRU eviction
//! phase for that tick (by substituting `max_total_events = usize::MAX`). The
//! next 60-second tick retries from scratch with a fresh scan. This ensures no
//! below-floor event is evicted when the scan was incomplete.
//!
//! Extracted from `ram_eviction.rs` to keep that file under the 500-LOC hard
//! cap (AGENTS.md file-size rule).

use std::collections::HashSet;
use std::ops::ControlFlow;

use crate::planner::InterestShape;
use crate::store::{EventStore, StoreQuery};

/// Per-call event-visit budget for [`pin_shape_events_below_floor`].
///
/// Mirrors `GC_MAX_EVENTS_PER_STEP` (2 000): the whole GC tick (pre-scan +
/// store step) stays within a comparable wall-clock envelope. For
/// `AuthorKind`/`KindDtag` the index `until` bound naturally limits results;
/// for `Etag`/`Ptag` (no index bound) this cap is the sole early-exit.
///
/// The value is deliberately conservative. Typical active Etag/Ptag shapes
/// should have far fewer than 2,000 matching events, so in the common case the
/// cap is never reached. When it IS reached the tick safely defers durable LRU
/// eviction.
pub(super) const PIN_SCAN_MAX_EVENTS: usize = 2_000;

/// Result of a single [`pin_shape_events_below_floor`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PinScanOutcome {
    /// All matching events were visited; the pin set is complete for this shape.
    Complete,
    /// The scan hit the `max_events` budget before finishing. The caller must
    /// treat this conservatively (skip LRU eviction this tick).
    Truncated,
}

/// Add to `pins` the store id of every event matching `shape` with
/// `created_at <= floor` (#1090 Stage 2 floor-coherence).
///
/// ## K3 Stage C — single shape→query mapping (ADR-0056 §3)
///
/// Derives its queries from the SAME [`compile_store_query_plan`] mapping
/// `shape_floor` (and the live `watermark_fn`) read, then pins every match at or
/// below `floor`. Queries that carry an `until` cursor (`AuthorKind`,
/// `KindDtag`) push the `<= floor` bound into the index scan; cursor-less
/// (`Etag`/`Ptag`) queries enumerate all matches and filter in the visitor.
/// Zero-author `KindTime` global feeds are never floored (so `shape_floor`
/// returns `None` and this is never reached for them); they are skipped
/// defensively if ever present.
///
/// Before Stage C this was a hand-rolled THIRD copy of the shape→`StoreQuery`
/// mapping kept "in lockstep" with `shape_floor` by comment; routing it through
/// `compile_store_query_plan` removes that drift hazard.
///
/// ## Scan budget (#1348 — D8 fix)
///
/// `max_events` caps the total number of events visited across all sub-queries
/// for this shape. When exhausted the function returns
/// [`PinScanOutcome::Truncated`] **without** having pinned the remaining
/// events. The caller must then skip LRU eviction for this tick (conservative
/// safety: we cannot evict what we may not have pinned). For `AuthorKind` and
/// `KindDtag` the index `until` bound naturally limits candidates; the cap
/// therefore primarily protects against large `Etag`/`Ptag` result sets.
pub(super) fn pin_shape_events_below_floor(
    shape: &InterestShape,
    floor: u64,
    store: &dyn EventStore,
    pins: &mut HashSet<crate::store::EventId>,
    max_events: usize,
) -> PinScanOutcome {
    use super::super::cache_serve::{compile_store_query_plan, query_since_mut, query_until_mut};

    let mut remaining = max_events;

    // Visit a query, pinning every event whose `created_at <= floor`.
    // Returns `true` if the scan completed within budget, `false` if truncated.
    //
    // We request `*rem + 1` results from `query_visit`: if we receive more than
    // `*rem` events the query had additional matches beyond the budget (truncated).
    // The extra event is never pinned — it is just a sentinel for "more results".
    let mut visit = |q: &StoreQuery, enforce_floor_in_visitor: bool, rem: &mut usize| -> bool {
        let limit = rem.saturating_add(1); // "+1 sentinel" to detect overflow
        let mut visited = 0usize;
        let _ = store.query_visit(q, limit, &mut |ev| {
            visited += 1;
            if visited > *rem {
                // Sentinel hit: there are more events than the budget allows.
                // Do not pin this event; break to signal truncation.
                return ControlFlow::Break(());
            }
            if !enforce_floor_in_visitor || ev.raw.created_at <= floor {
                if let Some(id) = ev.raw.id_bytes() {
                    pins.insert(id);
                }
            }
            ControlFlow::Continue(())
        });
        if visited > *rem {
            *rem = 0;
            false // truncated
        } else {
            *rem = rem.saturating_sub(visited);
            true // complete
        }
    };

    let Ok(plan) = compile_store_query_plan(shape) else {
        return PinScanOutcome::Complete;
    };
    for mut q in plan.queries {
        // Zero-author global feed: never floored (skip; defensive — the caller
        // only reaches here for shapes `shape_floor` returned `Some` for).
        if matches!(q, StoreQuery::KindTime { .. }) {
            continue;
        }
        // Clear `since` BEFORE applying the `<= floor` bound, exactly mirroring
        // `shape_floor`'s probe normalization above. `compile_store_query_plan`
        // embeds `shape.since`; a shape with `shape.since = Some(T)` where
        // `T > floor` would otherwise run an inverted range
        // `{ since: Some(T), until: Some(floor) }` → the store returns ZERO
        // events → the scan vacuously reports `Complete` → below-floor events go
        // unpinned → LRU eviction drops them → a permanent floor-coherence hole.
        // The floor is enforced via `until` = floor (cursored) or in the visitor
        // (cursor-less); `since` MUST be `None` so the scan reaches every
        // below-floor event. (K3 #1380 Bug 2.)
        if let Some(since) = query_since_mut(&mut q) {
            *since = None;
        }
        // Cursored queries (`AuthorKind`/`KindDtag`) push the `<= floor` bound
        // into the index; cursor-less (`Etag`/`Ptag`) enforce it in the visitor.
        let enforce_in_visitor = match query_until_mut(&mut q) {
            Some(until) => {
                *until = Some(floor);
                false
            }
            None => true,
        };
        if !visit(&q, enforce_in_visitor, &mut remaining) {
            return PinScanOutcome::Truncated;
        }
    }
    PinScanOutcome::Complete
}
