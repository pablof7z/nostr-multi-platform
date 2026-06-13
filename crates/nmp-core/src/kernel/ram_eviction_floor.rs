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
//! shape at or below that floor so `derive_store_pin_set` can pin them. The two
//! floor computations MUST stay in lockstep — any shape the `watermark_fn`
//! floors, `shape_floor` must floor identically, and vice versa (see
//! `cache_serve_budget_tests` for the floored⇒served guard).
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
/// This is the single source of truth for the floor the subscription
/// `watermark_fn` (`kernel/mod.rs`) installs: the newest stored event matching
/// the shape. It is factored out here so `derive_store_pin_set` pins exactly
/// the events the floor would otherwise strand.
///
/// Returns `None` for shapes the `watermark_fn` refuses to floor (id-pointer
/// shapes, no-kind shapes, multi-tag/multi-value shapes, zero-author kind-only
/// shapes, and any author with no stored events) — an unfloored shape needs no
/// floor-coherent pin because the relay re-sends its full history.
pub(super) fn shape_floor(shape: &InterestShape, store: &dyn EventStore) -> Option<u64> {
    // id-pointer shapes: one-shot loads, not floored.
    if !shape.event_ids.is_empty() {
        return None;
    }
    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();
    if kinds.is_empty() {
        return None;
    }

    // Address-pointer (NaddrCoord → KindDtag): newest matching any coord.
    if !shape.addresses.is_empty() {
        let mut newest: Option<u64> = None;
        for coord in &shape.addresses {
            let q = StoreQuery::KindDtag {
                kind: coord.kind,
                d_tag: coord.d_tag.as_bytes().to_vec(),
                since: None,
                until: None,
            };
            let mut ts: Option<u64> = None;
            let _ = store.query_visit(&q, 1, &mut |ev| {
                ts = Some(ev.raw.created_at);
                ControlFlow::Break(())
            });
            if let Some(t) = ts {
                newest = Some(newest.map_or(t, |prev| prev.max(t)));
            }
        }
        return newest;
    }

    // Single-tag-single-value (Etag / Ptag).
    if !shape.tags.is_empty() {
        if shape.tags.len() != 1 {
            return None;
        }
        let (tag_key, values) = shape.tags.iter().next()?;
        if values.len() != 1 {
            return None;
        }
        let target_hex = values.iter().next()?;
        let target = super::super::hex_to_pubkey_bytes(target_hex)?;
        let q = if tag_key == "e" {
            StoreQuery::Etag {
                target,
                kinds: kinds.clone(),
            }
        } else if tag_key == "p" {
            StoreQuery::Ptag {
                target,
                kinds: kinds.clone(),
            }
        } else {
            return None;
        };
        let mut ts: Option<u64> = None;
        let _ = store.query_visit(&q, 1, &mut |ev| {
            ts = Some(ev.raw.created_at);
            ControlFlow::Break(())
        });
        return ts;
    }

    // Author+kind: min over per-author newest; any author with no stored
    // events → None (their history must be fetched in full, so no floor).
    if shape.authors.is_empty() {
        return None;
    }
    let mut min_ts: Option<u64> = None;
    for author_hex in &shape.authors {
        let author = super::super::hex_to_pubkey_bytes(author_hex)?;
        let q = StoreQuery::AuthorKind {
            author,
            kinds: kinds.clone(),
            since: None,
            until: None,
        };
        let mut ts: Option<u64> = None;
        let _ = store.query_visit(&q, 1, &mut |ev| {
            ts = Some(ev.raw.created_at);
            ControlFlow::Break(())
        });
        let author_ts = ts?;
        min_ts = Some(match min_ts {
            None => author_ts,
            Some(prev) => prev.min(author_ts),
        });
    }
    min_ts
}

/// Add to `pins` the store id of every event matching `shape` with
/// `created_at <= floor` (#1090 Stage 2 floor-coherence).
///
/// Mirrors the shape→`StoreQuery` mapping in [`shape_floor`]. Queries that
/// support an `until` bound (`AuthorKind`, `KindDtag`) push the `<= floor`
/// filter into the index scan; `Etag`/`Ptag` (no `until` field) enumerate all
/// matches and filter in the visitor.
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
    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();
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

    if !shape.addresses.is_empty() {
        for coord in &shape.addresses {
            let q = StoreQuery::KindDtag {
                kind: coord.kind,
                d_tag: coord.d_tag.as_bytes().to_vec(),
                since: None,
                until: Some(floor),
            };
            if !visit(&q, false, &mut remaining) {
                return PinScanOutcome::Truncated;
            }
        }
        return PinScanOutcome::Complete;
    }

    if !shape.tags.is_empty() {
        // `shape_floor` only returns `Some` for single-tag-single-value e/p
        // shapes, so these lookups are infallible here.
        let Some((tag_key, values)) = shape.tags.iter().next() else {
            return PinScanOutcome::Complete;
        };
        let Some(target_hex) = values.iter().next() else {
            return PinScanOutcome::Complete;
        };
        let Some(target) = super::super::hex_to_pubkey_bytes(target_hex) else {
            return PinScanOutcome::Complete;
        };
        let q = if tag_key == "e" {
            StoreQuery::Etag {
                target,
                kinds: kinds.clone(),
            }
        } else if tag_key == "p" {
            StoreQuery::Ptag {
                target,
                kinds: kinds.clone(),
            }
        } else {
            return PinScanOutcome::Complete;
        };
        // Etag/Ptag have no `until` field — enforce the floor in the visitor
        // and apply the count budget (the sole D8 guard for these query types).
        if !visit(&q, true, &mut remaining) {
            return PinScanOutcome::Truncated;
        }
        return PinScanOutcome::Complete;
    }

    // Author+kind.
    for author_hex in &shape.authors {
        let Some(author) = super::super::hex_to_pubkey_bytes(author_hex) else {
            continue;
        };
        let q = StoreQuery::AuthorKind {
            author,
            kinds: kinds.clone(),
            since: None,
            until: Some(floor),
        };
        if !visit(&q, false, &mut remaining) {
            return PinScanOutcome::Truncated;
        }
    }
    PinScanOutcome::Complete
}
