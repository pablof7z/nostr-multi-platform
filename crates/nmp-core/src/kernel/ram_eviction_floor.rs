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
//! Extracted from `ram_eviction.rs` to keep that file under the 500-LOC hard
//! cap (AGENTS.md file-size rule).

use std::collections::HashSet;
use std::ops::ControlFlow;

use crate::planner::InterestShape;
use crate::store::{EventStore, StoreQuery};

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
/// matches and filter in the visitor. The scan is unbounded (`usize::MAX`) —
/// it must enumerate every below-floor event, not just the newest.
pub(super) fn pin_shape_events_below_floor(
    shape: &InterestShape,
    floor: u64,
    store: &dyn EventStore,
    pins: &mut HashSet<crate::store::EventId>,
) {
    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();

    // Visit a query, pinning every event whose `created_at <= floor`.
    let mut visit = |q: &StoreQuery, enforce_floor_in_visitor: bool| {
        let _ = store.query_visit(q, usize::MAX, &mut |ev| {
            if !enforce_floor_in_visitor || ev.raw.created_at <= floor {
                if let Some(id) = ev.raw.id_bytes() {
                    pins.insert(id);
                }
            }
            ControlFlow::Continue(())
        });
    };

    if !shape.addresses.is_empty() {
        for coord in &shape.addresses {
            let q = StoreQuery::KindDtag {
                kind: coord.kind,
                d_tag: coord.d_tag.as_bytes().to_vec(),
                since: None,
                until: Some(floor),
            };
            visit(&q, false);
        }
        return;
    }

    if !shape.tags.is_empty() {
        // `shape_floor` only returns `Some` for single-tag-single-value e/p
        // shapes, so these lookups are infallible here.
        let Some((tag_key, values)) = shape.tags.iter().next() else {
            return;
        };
        let Some(target_hex) = values.iter().next() else {
            return;
        };
        let Some(target) = super::super::hex_to_pubkey_bytes(target_hex) else {
            return;
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
            return;
        };
        // Etag/Ptag have no `until` field — enforce the floor in the visitor.
        visit(&q, true);
        return;
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
        visit(&q, false);
    }
}
