//! The filter-merge lattice: `merge()` implements Rules 1–11 from the compiler
//! design. Only shapes that pass all eleven rules are merged; otherwise the
//! caller emits two distinct REQs.
//!
//! ## Module structure
//!
//! - `rules` — the individual rule functions (pub(super)).
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.3
//! Doctrine: D8 (zero per-event allocs on the hot path after warmup).
//!
//! ## Rules summary
//! 1. `kinds` — equal or one wildcard; wildcard absorbs.
//! 2. `tags` — same key dimensions; per-dimension value union ≤ limit
//!    (the "h-tag coalesce" workhorse: when two shapes share a `relay_pin`,
//!    this is what collapses their per-room tag values into one REQ).
//! 3. `since` — `min(a, b)` iff both present or both absent; mixed = refuse.
//! 4. `until` — `max(a, b)` iff both present or both absent; mixed = refuse.
//! 5. `limit` — merge only if both absent.
//! 6. `lifecycle` — identical lifecycles only.
//! 7. `event_ids` — union, capped.
//! 8. `addresses` — union, capped; requires other fields mergeable per 1–7.
//! 9. `relay_pin` — host-relay-pin equality; `None` does NOT absorb `Some(_)`.
//!    Generic third-routing-lane contract for any protocol that requires
//!    addressing a single host relay.
//! 10. `search` — equality; relay NIP-50 filters must not merge with
//!     non-search filters or different search strings.
//! 11. `p_tag_routing` — equality; NIP-17 DM-relay inbox routing must not
//!     merge with generic NIP-65 `#p` routing.

mod rules;

use crate::interest::{InterestLifecycle, InterestShape};
use rules::{
    rule10_search, rule1_kinds, rule2_tags, rule3_since, rule4_until, rule5_limit, rule6_lifecycle,
    rule7_event_ids, rule8_addresses, rule9_relay_pin,
};

/// Per-relay cap for merged value sets (tags, ids, addresses).
/// This mirrors the relay default of 1000 per filter.
const DEFAULT_VALUE_LIMIT: usize = 1000;

/// Outcome of attempting to merge two `InterestShape`s on a single relay.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeOutcome {
    /// Shapes were merged; the returned shape is the result.
    Merged(InterestShape),
    /// Shapes cannot be merged without changing semantics.
    Refused,
}

/// Merges two filter shapes into one.
///
/// Attempt to merge shape `b` into shape `a` on a given relay.
///
/// Returns `Merged(result)` iff all 11 rules pass; `Refused` otherwise.
/// Neither `a` nor `b` is modified on refusal.
///
/// # ⚠ Superset semantics
///
/// The merged shape is a **superset** of both inputs — it matches everything
/// either input matches, *plus* combinations neither input asked for (e.g.,
/// cross-products of author sets and tag sets). This is correct for
/// wire-coalescing (fewer REQs is better), but callers must not assume the
/// merged sub-shape is a tight filter. Store ingest applies author-gating
/// independently to filter over-delivered events.
///
/// Design: §3.3 Rules 1–11
#[must_use]
pub fn merge(
    a: &InterestShape,
    b: &InterestShape,
    lifecycle_a: &InterestLifecycle,
    lifecycle_b: &InterestLifecycle,
) -> MergeOutcome {
    // Rule 6 first — cheapest check, prune early.
    if !rule6_lifecycle(lifecycle_a, lifecycle_b) {
        return MergeOutcome::Refused;
    }

    // Rule 9 second — also cheap (Option equality), and a refusal here means
    // the two interests will definitely be sent to different relays. Pruning
    // before the more expensive set unions saves work on host-pinned views.
    if !rule9_relay_pin(a, b) {
        return MergeOutcome::Refused;
    }

    // Rule 10 — relay NIP-50 search. This is a wire filter field, but unlike
    // kinds/tags it has no safe broadening rule.
    if !rule10_search(a, b) {
        return MergeOutcome::Refused;
    }

    // Rule 11 — p-tag routing mode. This is not a wire filter field, but it
    // decides which relay set is used for Case C; keep merged shapes tied to
    // one routing policy.
    if a.p_tag_routing != b.p_tag_routing {
        return MergeOutcome::Refused;
    }

    // Rule 1 — kinds
    let Some(merged_kinds) = rule1_kinds(a, b) else {
        return MergeOutcome::Refused;
    };

    // Rule 2 — tag dimensions
    let Some(merged_tags) = rule2_tags(a, b, DEFAULT_VALUE_LIMIT) else {
        return MergeOutcome::Refused;
    };

    // Rule 3 — since
    let Some(merged_since) = rule3_since(a, b) else {
        return MergeOutcome::Refused;
    };

    // Rule 4 — until
    let Some(merged_until) = rule4_until(a, b) else {
        return MergeOutcome::Refused;
    };

    // Rule 5 — limit
    if !rule5_limit(a, b) {
        return MergeOutcome::Refused;
    }

    // Rule 7 — event_ids union
    let Some(merged_event_ids) = rule7_event_ids(a, b, DEFAULT_VALUE_LIMIT) else {
        return MergeOutcome::Refused;
    };

    // Rule 8 — addresses union (requires prior rules to have passed)
    let Some(merged_addresses) = rule8_addresses(a, b, DEFAULT_VALUE_LIMIT) else {
        return MergeOutcome::Refused;
    };

    MergeOutcome::Merged(InterestShape {
        authors: a.authors.union(&b.authors).cloned().collect(),
        kinds: merged_kinds,
        tags: merged_tags,
        since: merged_since,
        until: merged_until,
        limit: None, // Rule 5 guarantees both are None
        search: a.search.clone(),
        event_ids: merged_event_ids,
        addresses: merged_addresses,
        // Rule 9 guaranteed equality above; either side carries the result.
        relay_pin: a.relay_pin.clone(),
        // Rule 11 guaranteed equality above; either side carries the result.
        p_tag_routing: a.p_tag_routing,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod relay_search_tests;
#[cfg(test)]
mod tests;
