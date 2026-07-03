//! Wire-event handlers + read-only plan diagnostics.
//!
//! Split out of `subs/mod.rs` (file-size-gate, NMP #169) with zero
//! behavioural change. Holds the reconnect / EOSE / deadline / auth-state
//! handlers and the `current_plan_*` diagnostic accessors. Sibling child
//! module of `subs`, so `SubscriptionLifecycle`'s private fields stay
//! reachable.

use std::sync::Arc;

use crate::planner::{InterestId, InterestLifecycle, RelayUrl};

use super::trigger::RelayAuthState;
use super::watermark_rewrite::shape_is_ephemeral_only;
use super::wire::{self, lifecycle_for_shape, WireFrame};
use super::SubscriptionLifecycle;

impl SubscriptionLifecycle {
    /// Materialise the full current plan as `WireFrame::Req`s — one per
    /// `(relay, sub_shape)` — independent of the prior-plan diff.
    ///
    /// `recompile_and_diff` returns only the *delta* vs. the last plan, so
    /// once the plan stabilises a recompile yields few or no frames even
    /// though live subscriptions exist. Diagnostics need the complete
    /// in-effect REQ set without tearing the registry down and
    /// rebuilding it (which would double-count `compile_count` and re-fire
    /// the lifecycle / auth gates). This is the read-only seam for that.
    ///
    /// Probe REQs are intentionally absent: implicit kind:10002 discovery
    /// frames are appended *outside* `current_plan` (see
    /// [`Self::recompile_and_diff`]), so the returned vec is content-only by
    /// construction.
    #[must_use]
    pub fn current_plan_frames(&self) -> Vec<WireFrame> {
        let Some(plan) = self.current_plan.as_ref() else {
            return Vec::new();
        };
        let interests = self.registry.iter_active();
        let mut frames = Vec::new();
        for (relay_url, relay_plan) in &plan.per_relay {
            for shape in &relay_plan.sub_shapes {
                let interest_id = shape
                    .originating_interests
                    .first()
                    .cloned()
                    .unwrap_or(InterestId(0));
                frames.push(WireFrame::Req {
                    relay_url: relay_url.clone(),
                    sub_id: wire::sub_id_for(&plan.plan_id, shape),
                    filter_json: wire::filter_json_for(&shape.shape),
                    interest_id,
                    lifecycle: wire::lifecycle_for_shape(shape, &interests),
                });
            }
        }
        frames
    }

    /// Authors the last `recompile_and_diff` could not route to any relay
    /// (no cached NIP-65 mailbox, no app-relay substitute). Empty when no
    /// compile has run yet.
    ///
    /// This is the read-only seam onto the otherwise-internal
    /// `CompiledPlan::unroutable_authors` — exposed for diagnostics without
    /// leaking the whole plan. Recomputing this caller-side would mean re-walking the
    /// mailbox cache against the interest author set; the plan already did
    /// that work, so prefer this accessor.
    #[must_use]
    pub fn current_plan_unroutable(&self) -> std::collections::BTreeSet<String> {
        self.current_plan
            .as_ref()
            .map(|p| p.unroutable_authors.clone())
            .unwrap_or_default()
    }

    /// Current planner-originating interests keyed by `(relay_url, wire_id)`.
    ///
    /// Wire subscription rows keep transport facts; the compiled plan owns the
    /// logical-interest provenance for each `SubShape`. Diagnostics join the two
    /// here without storing planner internals in the transport row.
    #[must_use]
    pub(crate) fn current_plan_wire_origins(
        &self,
    ) -> std::collections::BTreeMap<(RelayUrl, String), Vec<InterestId>> {
        let Some(plan) = self.current_plan.as_ref() else {
            return std::collections::BTreeMap::new();
        };
        let mut origins = std::collections::BTreeMap::new();
        for (relay_url, relay_plan) in &plan.per_relay {
            for shape in &relay_plan.sub_shapes {
                origins.insert(
                    (relay_url.clone(), wire::sub_id_for(&plan.plan_id, shape)),
                    shape.originating_interests.clone(),
                );
            }
        }
        origins
    }

    /// Current registry owner refcounts keyed by planner interest id.
    ///
    /// Read by diagnostic projections that need logical consumer counts while
    /// preserving the registry as the single writer of logical-interest state.
    #[must_use]
    pub(crate) fn current_interest_owner_counts(
        &self,
    ) -> std::collections::BTreeMap<InterestId, usize> {
        self.registry.owner_counts_by_interest_id()
    }

    /// Per-relay routing provenance from the last successful compile, captured
    /// BEFORE the blocked-relay post-pass (SPLIT A). Empty before the first
    /// compile. Read by `Kernel::relay_diagnostics_snapshot` to surface
    /// connection reasons for all selected relays — including blocked ones.
    #[must_use]
    pub fn current_plan_attribution(
        &self,
    ) -> &std::collections::BTreeMap<crate::planner::RelayUrl, crate::planner::RelayAttribution>
    {
        &self.current_plan_attribution
    }

    /// A5 — relay-reconnected. Per recompilation.md §4.2: replay current plan
    /// to that relay WITHOUT invoking the planner. This is a pure replay, not
    /// a recompile.
    ///
    /// T116/G1 wiring point: the actor calls this on `RelayEvent::Connected`
    /// when the URL has been seen before (i.e. a true reconnect, not a first
    /// dial). Returned frames are fresh REQs that re-establish every active
    /// sub-shape that targeted this URL in the last `current_plan`.
    ///
    /// T129 watermark on replay: between the last `recompile_and_diff` and
    /// this reconnect the store may have ingested newer events. We
    /// re-apply the watermark per-shape *on a clone* so the REQ does not
    /// re-fetch already-stored events. Per recompilation.md §4.2 "this is a
    /// pure replay, not a recompile" — we deliberately do NOT mutate
    /// `current_plan`; only the on-the-wire `since` is bumped. This keeps
    /// `sub_id` stability (`canonical_filter_hash` is computed off `shape` not
    /// the post-watermark filter — see `planner/mod.rs::canonical_filter_hash`
    /// rationale and the T129 carve-out in `apply_watermark_rewrite`).
    #[must_use]
    pub fn handle_reconnect(&mut self, relay_url: RelayUrl) -> Vec<WireFrame> {
        let Some(plan) = self.current_plan.as_ref() else {
            return Vec::new();
        };
        let Some(relay_plan) = plan.per_relay.get(&relay_url) else {
            return Vec::new();
        };
        let interests = self.registry.iter_active();
        let watermark_fn = self.watermark_fn.as_ref().map(Arc::clone);
        let mut frames = Vec::with_capacity(relay_plan.sub_shapes.len());
        for shape in &relay_plan.sub_shapes {
            let sub_id = wire::sub_id_for(&plan.plan_id, shape);
            let interest_id = shape
                .originating_interests
                .first()
                .cloned()
                .unwrap_or(InterestId(0));
            let lifecycle = wire::lifecycle_for_shape(shape, &interests);
            let filter_json = match watermark_fn.as_ref() {
                Some(wm) if !shape_is_ephemeral_only(&shape.shape) => {
                    let mut wire_shape = shape.shape.clone();
                    // #1281 (lifecycle-aware): mirror apply_watermark_rewrite's
                    // semantics on the reconnect-replay path.
                    //
                    // - Tailing + since=None → set since=watermark+1 (live feed,
                    //   skip already-cached events; T129 narrowing).
                    // - Non-Tailing + since=None → leave None (backfill/oneshot,
                    //   full history requested; exempt from T129).
                    // - since=Some(t) → raise floor to max(t, watermark+1).
                    // K3 Stage D2 (ADR-0072 §3.D2): the floor is per-`(filter_hash,
                    // relay)`. This reconnect-replay path already has the target
                    // `relay_url` in scope (the method arg), so thread it into the
                    // resolver exactly as `apply_watermark_rewrite` does — both
                    // floor-application sites must read the ledger by the same key.
                    if wire_shape.since.is_none() {
                        let lc = lifecycle_for_shape(shape, &interests);
                        if lc == InterestLifecycle::Tailing {
                            if let Some(watermark) = wm(&wire_shape, &relay_url) {
                                wire_shape.since = Some(watermark.saturating_add(1));
                            }
                        }
                        // non-Tailing: leave since=None (backfill exemption).
                    } else if let Some(existing) = wire_shape.since {
                        if let Some(watermark) = wm(&wire_shape, &relay_url) {
                            let floor = watermark.saturating_add(1);
                            if floor > existing {
                                wire_shape.since = Some(floor);
                            }
                        }
                    }
                    wire::filter_json_for(&wire_shape)
                }
                _ => wire::filter_json_for(&shape.shape),
            };
            frames.push(WireFrame::Req {
                relay_url: relay_url.clone(),
                sub_id,
                filter_json,
                interest_id,
                lifecycle,
            });
        }
        frames
    }

    /// A9 — auth state transitioned. On `Authenticated`, flush any pending
    /// REQs held for that relay; on `ChallengeReceived`/`Authenticating`,
    /// future REQs for the relay will be diverted to the pending buffer.
    pub fn handle_auth_state_change(
        &mut self,
        relay_url: RelayUrl,
        state: RelayAuthState,
    ) -> Vec<WireFrame> {
        self.auth_gate.record_transition(relay_url, state)
    }

    /// T148 — test-only inspection of the AuthGate's per-URL pause predicate.
    /// Pins the per-URL keying invariant: a challenge that arrived on URL_B
    /// must NOT pause URL_A. See `kernel/auth_url_threading_tests.rs`.
    #[cfg(test)]
    pub(crate) fn is_auth_paused_for_url(&self, relay_url: &str) -> bool {
        self.auth_gate.is_paused(relay_url)
    }
}
