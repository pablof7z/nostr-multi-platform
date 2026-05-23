//! Individual merge rule implementations for the filter-merge lattice.
//!
//! Each function corresponds to one rule from compiler.md §3.3.
//! All rules are `pub(super)` — only the lattice `merge()` entry point is public.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.3
//! Doctrine: D8 (zero per-event allocs on the hot path after warmup).

use crate::planner::interest::{InterestLifecycle, InterestShape, NaddrCoord};

/// Rule 1 — `kinds` merge.
///
/// Mergeable iff `a.kinds == b.kinds` OR one is empty (wildcard absorbs ALL).
///
/// An empty set means "match any kind" (wildcard). When either side is wildcard,
/// the result MUST be wildcard (empty), not the other side's concrete set.
/// Returning the concrete set would NARROW the subscription semantics — a relay
/// receiving `{ kinds: [1, 6] }` would miss kinds 0, 30023, etc. that the
/// wildcard side intended to include.
///
/// `wildcard ∪ {1, 6} = wildcard` — the wildcard absorbs its neighbour.
pub(super) fn rule1_kinds(
    a: &InterestShape,
    b: &InterestShape,
) -> Option<std::collections::BTreeSet<u32>> {
    if a.kinds.is_empty() || b.kinds.is_empty() {
        // At least one side is wildcard — wildcard absorbs, result is wildcard.
        Some(std::collections::BTreeSet::new())
    } else if a.kinds == b.kinds {
        Some(a.kinds.clone())
    } else {
        // Both non-empty but different — refuse (merging would widen kinds)
        None
    }
}

/// Rule 2 — `tags` merge.
///
/// Mergeable iff both shapes have the same tag key dimensions, AND the union
/// of values per dimension stays under `limit`.
pub(super) fn rule2_tags(
    a: &InterestShape,
    b: &InterestShape,
    limit: usize,
) -> Option<std::collections::BTreeMap<crate::planner::interest::TagKey, std::collections::BTreeSet<String>>> {
    // Keys must be identical (same dimensions)
    if a.tags.keys().ne(b.tags.keys()) {
        return None;
    }

    let mut merged = std::collections::BTreeMap::new();
    for (key, av) in &a.tags {
        let bv = b.tags.get(key)?;
        let union: std::collections::BTreeSet<String> = av.union(bv).cloned().collect();
        if union.len() > limit {
            return None;
        }
        merged.insert(key.clone(), union);
    }
    Some(merged)
}

/// Rule 3 — `since` merge.
///
/// Returns `min(a.since, b.since)` iff both are `Some` or both are `None`.
/// Mixed (one bounded, one unbounded) returns `None` (refuse).
#[allow(clippy::option_option)] // Outer None = merge refused; Some(None) = unbounded; Some(Some(x)) = bounded
pub(super) fn rule3_since(a: &InterestShape, b: &InterestShape) -> Option<Option<u64>> {
    match (a.since, b.since) {
        (None, None) => Some(None),
        (Some(sa), Some(sb)) => Some(Some(sa.min(sb))),
        _ => None,
    }
}

/// Rule 4 — `until` merge.
///
/// Returns `max(a.until, b.until)` iff both are `Some` or both are `None`.
/// Mixed returns `None` (refuse).
#[allow(clippy::option_option)] // Outer None = merge refused; Some(None) = unbounded; Some(Some(x)) = bounded
pub(super) fn rule4_until(a: &InterestShape, b: &InterestShape) -> Option<Option<u64>> {
    match (a.until, b.until) {
        (None, None) => Some(None),
        (Some(ua), Some(ub)) => Some(Some(ua.max(ub))),
        _ => None,
    }
}

/// Rule 5 — `limit` merge.
///
/// Mergeable iff both limits are absent. If either has a limit, refuse
/// (broadening would mask the limit's intent).
pub(super) fn rule5_limit(a: &InterestShape, b: &InterestShape) -> bool {
    a.limit.is_none() && b.limit.is_none()
}

/// Rule 6 — `lifecycle` merge.
///
/// Tailing and one-shot must not merge (one-shot would never close the tailing
/// subscription). Both lifecycles must be identical.
pub(super) fn rule6_lifecycle(a: &InterestLifecycle, b: &InterestLifecycle) -> bool {
    a == b
}

/// Rule 7 — `event_ids` merge by union.
///
/// Returns `None` if the union would exceed `limit`.
pub(super) fn rule7_event_ids(
    a: &InterestShape,
    b: &InterestShape,
    limit: usize,
) -> Option<std::collections::BTreeSet<crate::planner::interest::EventId>> {
    let union: std::collections::BTreeSet<_> = a.event_ids.union(&b.event_ids).cloned().collect();
    if union.len() > limit {
        None
    } else {
        Some(union)
    }
}

/// Rule 8 — `addresses` merge by union.
///
/// Merges the address-pointer sets. Returns `None` if the union exceeds `limit`.
/// The other constraints (authors, kinds, time, lifecycle) must have been
/// checked by Rules 1–7 before reaching this point.
pub(super) fn rule8_addresses(
    a: &InterestShape,
    b: &InterestShape,
    limit: usize,
) -> Option<std::collections::BTreeSet<NaddrCoord>> {
    let union: std::collections::BTreeSet<_> =
        a.addresses.union(&b.addresses).cloned().collect();
    if union.len() > limit {
        None
    } else {
        Some(union)
    }
}

/// Rule 9 — `relay_pin` equality (the "h-tag coalesce" lane).
///
/// Two shapes are mergeable on this dimension iff their `relay_pin` values are
/// *identical* (both `None`, or both `Some(same_url)`). A `None` does NOT
/// absorb a `Some(_)` — unlike Rule 1's wildcard for kinds, the routing pin is
/// a hard override that suppresses the four-lane routing entirely, so mixing
/// pinned + unpinned interests would either narrow the unpinned scope (if the
/// pin won) or leak the pinned content to other relays (if `None` won).
///
/// Two interests with `relay_pin = Some(host_a)` and `relay_pin = Some(host_b)`
/// where `host_a != host_b` go to *different relays* and therefore cannot be
/// merged into a single wire frame regardless of how compatible their other
/// fields are.
///
/// When two shapes DO share a host (`relay_pin = Some(same_host)`), the rest
/// of the lattice coalesces them — Rule 2's tag-value union is what collapses
/// many sub-room subscriptions (each carrying its own per-room tag filter
/// value) into a single per-host REQ. That is the generic "h-tag coalesce"
/// behavior the third routing lane is named after.
///
/// This is the lattice half of the relay-pin contract. The partition half
/// lives in `planner::compiler::partition::case_e_relay_pinned`.
pub(super) fn rule9_relay_pin(a: &InterestShape, b: &InterestShape) -> bool {
    a.relay_pin == b.relay_pin
}
