//! T129 watermark-rewrite helpers extracted from `recompile.rs` to satisfy the
//! 500-LOC file-size gate (AGENTS.md).
//!
//! [`shape_is_ephemeral_only`] and [`apply_watermark_rewrite`] remain
//! `pub(super)` — visible to every sibling child module of `subs`
//! (`recompile`, `handlers`, …) via `super::watermark_rewrite::*`.

use crate::planner::{CompiledPlan, InterestLifecycle, InterestShape, LogicalInterest};

use super::wire::lifecycle_for_shape;

/// Returns `true` when every kind in `shape.kinds` is in the ephemeral range
/// 20000..30000 (per NIP-01 §3 ephemerals). Empty `kinds` is "wildcard" and
/// is NOT considered ephemeral — persistent kinds may match, so the rewrite
/// still applies. Mirrors the carve-out NDK added in commit `5afbd245`.
pub(super) fn shape_is_ephemeral_only(shape: &InterestShape) -> bool {
    !shape.kinds.is_empty() && shape.kinds.iter().copied().all(nmp_kinds::is_ephemeral)
}

/// In-place rewrite of every non-ephemeral sub-shape's `since` to
/// `max(existing_since, watermark + 1)`.
///
/// The rewrite is lifecycle-aware (#1281 refinement):
///
/// - **`Tailing` + `since=None`**: the interest is a live feed that wants
///   events from now onward. The rewrite IS applied — `since` is set to
///   `watermark + 1` so the relay does not re-send already-cached events.
///   This is the core T129 optimisation for ongoing subscriptions.
///
/// - **non-`Tailing` (OneShot/backfill) + `since=None`**: the caller
///   explicitly requested full history ("all-time / backfill"). Raising
///   `None` to `watermark+1` would silently prevent the relay from returning
///   events older than the local store watermark, defeating backfill.
///   These interests are EXEMPT — `since` stays `None`.
///
/// - **`since=Some(t)` (any lifecycle)**: the optimisation always applies —
///   raise the existing floor to `max(t, watermark + 1)` so the relay does
///   not re-send events already on disk.
///
/// The `interests` slice is needed to resolve each sub-shape's lifecycle via
/// its `originating_interests` IDs (mirrors `wire::lifecycle_for_shape`).
///
/// The rewrite is purely a value mutation — `canonical_filter_hash` is left
/// untouched so the wire-emitter's diff treats a re-opened sub as the same
/// `sub_id` it had before (the watermark moves between recompiles, but the
/// REQ is only emitted on the first compile that introduces the shape).
/// This matches NDK's `opts.addSinceFromCache` once-at-sub-open semantics
/// (`core/src/subscription/index.ts:537`).
///
/// D8: walks the plan tree exactly once; no per-shape allocation beyond the
/// one closure call into the resolver (which itself is responsible for
/// reusing its index buffers via `query_visit(limit=1)`).
pub(super) fn apply_watermark_rewrite(
    plan: &mut CompiledPlan,
    watermark_fn: &(dyn Fn(&InterestShape, &str) -> Option<u64> + Send + Sync),
    interests: &[LogicalInterest],
) {
    // K3 Stage D2 (ADR-0072 §3.D2): the floor is computed per-`(filter_hash,
    // relay)`, not per-shape. The relay is the `per_relay` map key, in scope
    // here, so we thread `relay_plan.relay_url` into the resolver. The
    // presence-derived resolver ignores the relay (presence is relay-agnostic),
    // so this is behaviour-preserving until the coverage ledger is enabled with
    // a row for the key — the central plumbing change D1's body flagged.
    for relay_plan in plan.per_relay.values_mut() {
        let relay_url = relay_plan.relay_url.clone();
        for sub_shape in &mut relay_plan.sub_shapes {
            if shape_is_ephemeral_only(&sub_shape.shape) {
                continue;
            }
            if sub_shape.shape.since.is_none() {
                // #1281 (lifecycle-aware): only apply T129 narrowing for Tailing
                // interests. A Tailing+None interest is a live feed — we narrow it
                // to watermark+1 so the relay skips already-cached events.
                // A non-Tailing+None interest (backfill/oneshot) must stay None so
                // the relay returns full history, not just events newer than the
                // local watermark.
                let lifecycle = lifecycle_for_shape(sub_shape, interests);
                if lifecycle != InterestLifecycle::Tailing {
                    continue;
                }
                // Tailing + since=None: apply T129 narrowing.
                let Some(watermark) = watermark_fn(&sub_shape.shape, &relay_url) else {
                    continue;
                };
                sub_shape.shape.since = Some(watermark.saturating_add(1));
                continue;
            }
            // since=Some(t): raise the existing floor toward watermark+1.
            // The is_none() branch above continues, so since is always Some here.
            let Some(existing) = sub_shape.shape.since else {
                continue;
            };
            let Some(watermark) = watermark_fn(&sub_shape.shape, &relay_url) else {
                continue;
            };
            let floor = watermark.saturating_add(1);
            if floor > existing {
                sub_shape.shape.since = Some(floor);
            }
        }
    }
}
