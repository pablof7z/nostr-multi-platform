//! #1090 Stage 2 — floor-coherent store-scan helpers for
//! [`Kernel::derive_store_pin_set`](super::super::Kernel::derive_store_pin_set).
//!
//! ## The hole these helpers close
//!
//! The live `since`-floor for a subscription is content-derived: the kernel's
//! `watermark_fn` (`kernel/mod.rs`) floors each REQ's `since` to the newest
//! stored event matching the shape + 1, so the relay does not re-emit events
//! already on disk. LRU eviction (the Stage-3 `HOT_EVENT_CEILING`) is free to
//! delete a *middle* event older than the surviving newest event — the floor
//! stays at `newest + 1`, so the self-healing REQ never re-requests the evicted
//! middle event: a permanent hole.
//!
//! [`shape_floor`] computes the same floor the `watermark_fn` installs, and
//! [`pin_shape_events_below_floor`] enumerates every stored event matching the
//! shape at or below that floor so `derive_store_pin_set` can pin them.
//!
//! ## K3 Stage C — single shape→query mapping (ADR-0056 §3)
//!
//! Both helpers now read the SAME `shape_to_store_queries` mapping the live
//! `watermark_fn` reads (`shape_floor` via `watermark_from_queries`,
//! `pin_shape_events_below_floor` by iterating the mapping and applying the
//! `<= floor` bound). The lockstep is therefore STRUCTURAL — there is one
//! shape→`StoreQuery` mapping, not three hand-synced copies — so the floor can
//! never floor on a shape (or at a timestamp) the serve mapping would not.
//! `shape_floor` is byte-identical to the `watermark_fn` floor, including the
//! K3 Stage B1 MIN/abort addressable rule and the Stage B3 truncated-serve
//! refusal. (See `cache_serve_budget_tests` / `gc_floor_coherent_tests` for the
//! floored⇒served and floor⇄serve guards.)
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
use std::sync::{Arc, Mutex};

use crate::planner::InterestShape;
use crate::store::{EventStore, StoreQuery};

/// Snapshot the live `Kernel::etag_ptag_truncated_serves` set for one GC sweep.
///
/// A poisoned lock degrades to the empty set — i.e. no shape is treated as
/// truncated, the safe-degraded default (the pin floor then matches the
/// non-truncation arm rather than refusing a floor it cannot evaluate).
pub(super) fn truncated_serve_snapshot(set: &Arc<Mutex<HashSet<u64>>>) -> HashSet<u64> {
    set.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Per-call event-visit budget for [`pin_shape_events_below_floor`].
///
/// Mirrors `GC_MAX_EVENTS_PER_STEP` (2 000): the whole GC tick (pre-scan +
/// store step) stays within a comparable wall-clock envelope. For
/// `AuthorKind`/`KindDtag` the index `until` bound naturally limits results;
/// for `Etag`/`Ptag` (no index bound) this cap is the sole early-exit.
///
/// The value is deliberately conservative: a production store with
/// `HOT_EVENT_CEILING` (10 000) total events will rarely have more than a few
/// hundred events per Etag/Ptag shape, so in the common case the cap is never
/// reached. When it IS reached the tick safely defers LRU eviction.
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

/// Compute the content-derived `since`-floor for `shape` against `store`.
///
/// ## K3 Stage C — single shape→query mapping (ADR-0056 §3)
///
/// This routes through [`watermark_from_queries`] over the SAME
/// [`shape_to_store_queries`] mapping the live `watermark_fn` (`kernel/mod.rs`)
/// installs — "one mapping read two ways". It therefore returns the floor the
/// subscription `watermark_fn` would install **byte-identically**, including the
/// K3 Stage B1 MIN/abort-on-empty rule for multi-coord addressable shapes and
/// the Stage B3 truncated-serve refusal for cursor-less (`Etag`/`Ptag`) shapes.
///
/// Before Stage C this was a hand-rolled SECOND copy of the shape→`StoreQuery`
/// mapping that had already drifted: its addressable branch folded with
/// MAX-ignoring-empties (the pre-B1 unsafe policy) while `watermark_from_queries`
/// uses MIN/abort. Collapsing the two to one removes the migration hazard the
/// Stage D ledger swap must not inherit.
///
/// `truncated` is the live `Kernel::etag_ptag_truncated_serves` set so the floor
/// here refuses exactly the cursor-less shapes the `watermark_fn` refuses
/// (Stage B3) — keeping `shape_floor` and `watermark_fn` in exact lockstep, which
/// the floor-coherent pin set relies on (see `cache_serve_budget_tests`).
///
/// Returns `None` for shapes the `watermark_fn` refuses to floor (id-pointer
/// shapes, no-kind shapes, multi-tag/multi-value shapes, zero-author kind-only
/// shapes, any author/coord with no stored events, and truncated cursor-less
/// serves) — an unfloored shape needs no floor-coherent pin because the relay
/// re-sends its full history.
pub(super) fn shape_floor(
    shape: &InterestShape,
    store: &dyn EventStore,
    truncated: &HashSet<u64>,
) -> Option<u64> {
    use super::super::cache_serve::{query_since_mut, query_until_mut, watermark_from_queries};

    watermark_from_queries(
        shape,
        |q| {
            // Normalize to the watermark scan form (newest match regardless of
            // window), then read the newest stored `created_at`.
            let mut q = q.clone();
            if let Some(since) = query_since_mut(&mut q) {
                *since = None;
            }
            if let Some(until) = query_until_mut(&mut q) {
                *until = None;
            }
            let mut ts: Option<u64> = None;
            let _ = store.query_visit(&q, 1, &mut |ev| {
                ts = Some(ev.raw.created_at);
                ControlFlow::Break(())
            });
            ts
        },
        |key| truncated.contains(&key),
    )
}

/// Add to `pins` the store id of every event matching `shape` with
/// `created_at <= floor` (#1090 Stage 2 floor-coherence).
///
/// ## K3 Stage C — single shape→query mapping (ADR-0056 §3)
///
/// Derives its queries from the SAME [`shape_to_store_queries`] mapping
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
/// `shape_to_store_queries` removes that drift hazard.
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
    use super::super::cache_serve::{query_since_mut, query_until_mut, shape_to_store_queries};

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

    for mut q in shape_to_store_queries(shape) {
        // Zero-author global feed: never floored (skip; defensive — the caller
        // only reaches here for shapes `shape_floor` returned `Some` for).
        if matches!(q, StoreQuery::KindTime { .. }) {
            continue;
        }
        // Clear `since` BEFORE applying the `<= floor` bound, exactly mirroring
        // `shape_floor`'s probe normalization above. `shape_to_store_queries`
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
