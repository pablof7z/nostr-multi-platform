//! M8-subs — subscription lifecycle: the seam between M2's `CompiledPlan`
//! and the wire.
//!
//! See `docs/plan/m8-subscription-lifecycle.md` for the scope discriminator
//! against M4 (negentropy), M5 (NIP-42 auth), M7 (publishing). This module
//! ships only the **seams**:
//!
//! - [`InterestRegistry`] — single-writer logical-interest store (D4).
//! - [`CompileTrigger`] inbox — FIFO + per-tick coalescing (D8).
//! - Wire-emitter — `CompiledPlan` → `Vec<WireFrame>` diff.
//! - [`ConnectionPool`] — uniform send-path shared by M4/M5/M7 (D7).
//!
//! Doctrine:
//! - **D3** routing is consumed verbatim from the planner; we never reroute.
//! - **D4** the registry is the single writer of the active-interest set.
//! - **D6** all error types here are internal `Result`s; no FFI exposure.
//! - **D7** the pool reports; the actor decides. No reconnect policy here.
//! - **D8** per-tick coalesce caps recompiles at 1 / view / tick.
//!
//! Design: `docs/design/subscription-compilation/recompilation.md` §4.

pub(crate) mod auth_gate;
pub(crate) mod inbox;
pub(crate) mod lifecycle_gate;
pub(crate) mod oneshot;
pub(crate) mod pool;
pub(crate) mod registry;
#[cfg(test)]
mod since_rewrite_tests;
pub(crate) mod sub_key;
pub(crate) mod trigger;
pub(crate) mod unknown_ids;
pub(crate) mod wire;

use std::sync::Arc;

use auth_gate::AuthGate;
use lifecycle_gate::LifecycleGate;

use crate::planner::{
    CompiledPlan, InterestId, InterestShape, MailboxCache, PlannerError, RelayUrl,
    SubscriptionCompiler,
};

/// Post-compile plan-mutation hook (M4 negentropy coverage gate).
///
/// The lifecycle owns the *seam* into which `nmp-nip77`'s
/// `apply_coverage_filter` is installed by the actor at startup. The hook runs
/// between `compile()` and `plan_diff()` — i.e. after the M2 compiler
/// produces the plan but before the wire-emitter diffs against the prior
/// plan. The hook is free to drop sub-shapes, bump `since`, or otherwise
/// rewrite the plan; any sub-shape whose `shape` is mutated MUST call
/// [`crate::planner::SubShape::recompute_hash`] (see the M4 codex review's P1
/// finding in `docs/perf/codex-reviews/076173d.md`).
///
/// Direction: `nmp-core` defines the seam, `nmp-nip77` installs the policy —
/// keeping coverage-gate / NIP-77 vocabulary out of `nmp-core` per D0
/// ("kernel never grows app nouns").
pub type PlanCoverageHook = Arc<dyn Fn(&mut CompiledPlan) + Send + Sync>;

/// T129 watermark resolver — returns the most-recent stored `created_at`
/// (unix seconds) for events matching `shape`, or `None` when the store has
/// no matching events.
///
/// Installed by the kernel via [`SubscriptionLifecycle::set_watermark_fn`].
/// The kernel is the only legitimate caller — view modules and tests inject a
/// stub closure. The kernel-side closure translates the shape into a
/// `StoreQuery` (`AuthorKind` when authors+kinds are scoped, otherwise
/// `KindTime`) and invokes `EventStore::query_visit` with `limit = 1`, which
/// early-stops at the newest stored match on the relevant secondary index.
///
/// The trait-object signature keeps `nmp-core::subs` independent of any
/// concrete store type (D8: zero per-emit alloc, dispatch is a single vtable
/// lookup; the closure itself reuses the index buffers underlying
/// `query_visit`).
pub type WatermarkFn = Arc<dyn Fn(&InterestShape) -> Option<u64> + Send + Sync>;

pub use inbox::TriggerInbox;
pub use oneshot::{OneshotApi, OneshotToken};
pub use pool::{ConnectionPool, InMemoryPool, PoolSendOutcome};
pub use registry::InterestRegistry;
pub use sub_key::{SubIdentity, SubKey, SubKeyBuilder, SubOwnerKey, SubScope};
pub use unknown_ids::UnknownIds;
pub use trigger::{AccountId, CompileTrigger, InvalidateReason, RelayAuthState, SignerId};
pub use wire::{plan_diff, WireFrame};

// ─── SubscriptionLifecycle ───────────────────────────────────────────────────

/// The top-level subscription lifecycle controller.
///
/// Owns the registry, trigger inbox, last-compiled plan, the lifecycle gate
/// (OneShot / BoundedTime CLOSE bookkeeping), and the auth gate (REQs to
/// auth-paused relays held in a pending buffer). Drives recompiles when
/// ticked; emits `WireFrame`s for the actor to push through the connection
/// pool.
///
/// **Per-tick discipline (D8):** N triggers in the inbox between two
/// `drain_tick()` calls produce at most one compile. An empty inbox tick
/// produces zero compiles.
pub struct SubscriptionLifecycle {
    registry: InterestRegistry,
    inbox: TriggerInbox,
    indexer_relays: Vec<RelayUrl>,
    /// The plan currently believed-to-be-live on the wire.
    current_plan: Option<CompiledPlan>,
    /// Per-sub lifecycle bookkeeping (OneShot, BoundedTime).
    lifecycle_gate: LifecycleGate,
    /// Per-relay auth state + pending REQ buffer.
    auth_gate: AuthGate,
    /// Monotonic compile counter for test assertions.
    compile_count: u64,
    /// Optional post-compile plan-mutation hook (see [`PlanCoverageHook`]).
    /// Set via [`Self::set_coverage_hook`]; absent by default so the kernel
    /// links cleanly without any NIP-77 dependency.
    coverage_hook: Option<PlanCoverageHook>,
    /// T129 — optional watermark resolver. Installed by the kernel from the
    /// `EventStore` at startup; tests inject a stub closure. When set,
    /// [`Self::recompile_and_diff`] rewrites each non-ephemeral sub-shape's
    /// `since` to `max(existing_since, watermark + 1)` so the relay REQ does
    /// not re-fetch events already on disk. See module doc on [`WatermarkFn`]
    /// and the seam rationale documented in `planner/mod.rs`.
    watermark_fn: Option<WatermarkFn>,
}

impl Default for SubscriptionLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl SubscriptionLifecycle {
    /// Construct an empty lifecycle with a default indexer set.
    ///
    /// T132: the lifecycle no longer owns a mailbox cache. The caller passes a
    /// `&dyn MailboxCache` into `recompile_and_diff` / `drain_tick`, sourced
    /// from the kernel's `author_relay_lists` (via `KernelMailboxes`) in
    /// production, or an `InMemoryMailboxCache` constructed inline in tests.
    /// This eliminates the dual source-of-truth seam the planner-side cache
    /// previously created (T105 made `Kernel::author_relay_lists` authoritative).
    pub fn new() -> Self {
        Self {
            registry: InterestRegistry::new(),
            inbox: TriggerInbox::new(),
            indexer_relays: vec!["wss://purplepag.es".to_string()],
            current_plan: None,
            lifecycle_gate: LifecycleGate::new(),
            auth_gate: AuthGate::new(),
            compile_count: 0,
            coverage_hook: None,
            watermark_fn: None,
        }
    }

    /// Install (or replace) the post-compile [`PlanCoverageHook`].
    ///
    /// The actor calls this once at startup with
    /// `Arc::new(|plan| { nmp_nip77::apply_coverage_filter(plan, …); })`
    /// — `nmp-core` itself never knows the hook's identity. C10 is the
    /// `nmp-testing` contract test that exercises this seam end-to-end.
    pub fn set_coverage_hook(&mut self, hook: PlanCoverageHook) {
        self.coverage_hook = Some(hook);
    }

    /// T129 — install (or replace) the watermark resolver used by
    /// `addSinceFromCache`-style rewrites. The kernel constructs the closure
    /// at startup by capturing the `EventStore` handle and translating each
    /// `InterestShape` into a `StoreQuery` (`AuthorKind` when authors+kinds
    /// are scoped, otherwise `KindTime`); tests inject a deterministic stub.
    /// Without a resolver installed the rewrite is a no-op (legacy lifecycle
    /// tests stay green).
    ///
    /// The resolver is invoked synchronously inside `recompile_and_diff` and
    /// must therefore be cheap — implementations are expected to call
    /// `EventStore::query_visit` with `limit = 1`, which early-stops at the
    /// newest stored match on the relevant secondary index (no per-emit
    /// allocation; D8).
    pub fn set_watermark_fn(&mut self, f: WatermarkFn) {
        self.watermark_fn = Some(f);
    }

    /// Mutable access to the registry — view modules push interests through
    /// this in production; integration tests push directly.
    pub fn registry_mut(&mut self) -> &mut InterestRegistry {
        &mut self.registry
    }

    /// Compile counter (one increment per planner invocation).
    pub fn compile_count(&self) -> u64 {
        self.compile_count
    }

    /// Enqueue a trigger. Coalesced with siblings until the next `drain_tick`.
    pub fn enqueue_trigger(&mut self, trigger: CompileTrigger) {
        self.inbox.enqueue(trigger);
    }

    /// Recompile from current registry + caller-supplied mailbox state, diff
    /// against the last-compiled plan, and return the WireFrame delta.
    ///
    /// T132: the mailbox cache is no longer owned by the lifecycle. The kernel
    /// passes its `KernelMailboxes` adapter (a view onto `author_relay_lists`,
    /// populated by `ingest_relay_list` from real kind:10002 events); tests
    /// pass a local `InMemoryMailboxCache`. This eliminates the dual-source
    /// hazard the planner-side cache previously created.
    ///
    /// Updates the lifecycle gate; diverts REQs targeting auth-paused relays
    /// into the pending-auth buffer.
    pub fn recompile_and_diff(
        &mut self,
        mailbox_cache: &dyn MailboxCache,
    ) -> Result<Vec<WireFrame>, PlannerError> {
        let interests = self.registry.iter_active();
        let compiler = SubscriptionCompiler::new(mailbox_cache, &self.indexer_relays);
        let mut plan = compiler.compile(&interests)?;
        self.compile_count = self.compile_count.saturating_add(1);

        // D2 negentropy-first: let the coverage-gate hook (M4) rewrite the
        // plan before the wire-emitter sees it — skipping authoritative
        // (filter, relay) pairs and bumping `since` on pairs we already have
        // a watermark for. With no hook installed (the kernel-only path) the
        // plan flows through unchanged.
        if let Some(hook) = self.coverage_hook.as_ref() {
            hook(&mut plan);
        }

        // T129 — addSinceFromCache: rewrite each non-ephemeral shape's
        // `since` to `max(existing_since, watermark + 1)` so a freshly-opened
        // REQ does not re-fetch events the cache already has. Runs AFTER the
        // coverage hook so the two passes compose monotonically: coverage may
        // bump `since`, the watermark rewrite then raises it further if the
        // store has even fresher events. We intentionally do NOT recompute
        // `canonical_filter_hash` here — sub_id stability is the feature
        // (`planner/mod.rs::canonical_filter_hash` docs the rationale).
        if let Some(wm) = self.watermark_fn.as_ref() {
            apply_watermark_rewrite(&mut plan, wm.as_ref());
        }

        let prior = self.current_plan.as_ref();
        let raw_frames = plan_diff(prior, Some(&plan), &interests);

        // Update lifecycle bookkeeping BEFORE auth partition, so REQs held
        // back for auth are still considered "known" once they fire after
        // Authenticated drains the buffer.
        self.lifecycle_gate.observe_diff(&raw_frames);
        self.current_plan = Some(plan);

        Ok(self.auth_gate.partition(raw_frames))
    }

    /// Drain the trigger inbox at a tick boundary. Per D8, all triggers
    /// collapse into at most one compile pass; an empty inbox is a no-op.
    ///
    /// T132: the caller supplies the mailbox cache for the same reason
    /// [`Self::recompile_and_diff`] does — the lifecycle is no longer the
    /// owner of mailbox state.
    pub fn drain_tick(&mut self, mailbox_cache: &dyn MailboxCache) -> Vec<WireFrame> {
        let triggers = self.inbox.drain_coalesced();
        if triggers.is_empty() {
            return Vec::new();
        }
        // Apply non-recompile-side-effecting triggers first (auth-state). We
        // do not flush pending REQs here even on Authenticated; the
        // subsequent recompile will re-walk them as part of the new diff.
        for t in &triggers {
            if let CompileTrigger::RelayAuthStateChanged { url, state } = t {
                let _ = self.auth_gate.record_transition(url.clone(), state.clone());
            }
        }
        self.recompile_and_diff(mailbox_cache).unwrap_or_default()
    }

    /// A5 — relay-reconnected. Per recompilation.md §4.2: replay current plan
    /// to that relay WITHOUT invoking the planner. This is a pure replay, not
    /// a recompile.
    pub fn handle_reconnect(&mut self, relay_url: RelayUrl) -> Vec<WireFrame> {
        let Some(plan) = self.current_plan.as_ref() else {
            return Vec::new();
        };
        let Some(relay_plan) = plan.per_relay.get(&relay_url) else {
            return Vec::new();
        };
        let interests = self.registry.iter_active();
        let mut frames = Vec::with_capacity(relay_plan.sub_shapes.len());
        for shape in &relay_plan.sub_shapes {
            let sub_id = wire::sub_id_for(&plan.plan_id, shape);
            let interest_id = shape
                .originating_interests
                .first()
                .cloned()
                .unwrap_or(InterestId(0));
            let lifecycle = wire::lifecycle_for_shape(shape, &interests);
            let filter_json = wire::filter_json_for(&shape.shape);
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

    /// EOSE handler — closes OneShot subs, no-op for Tailing / BoundedTime.
    pub fn handle_eose(&mut self, relay_url: &str, sub_id: &str) -> Vec<WireFrame> {
        self.lifecycle_gate.on_eose(relay_url, sub_id)
    }

    /// Per-tick deadline check — closes BoundedTime subs whose `until_ms` has
    /// passed `now_ms`.
    pub fn tick_deadlines(&mut self, now_ms: u64) -> Vec<WireFrame> {
        self.lifecycle_gate.tick_deadlines(now_ms)
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

    /// Replace the indexer relay set (A8 trigger consumer). Used by M2 phase 2
    /// when user/operator changes the configured indexer list.
    #[allow(dead_code)]
    pub(crate) fn set_indexer_relays(&mut self, relays: Vec<RelayUrl>) {
        self.indexer_relays = relays;
    }
}

// ─── T129 watermark rewrite ──────────────────────────────────────────────────

/// Returns `true` when every kind in `shape.kinds` is in the ephemeral range
/// 20000..30000 (per NIP-01 §3 ephemerals). Empty `kinds` is "wildcard" and
/// is NOT considered ephemeral — persistent kinds may match, so the rewrite
/// still applies. Mirrors the carve-out NDK added in commit `5afbd245`.
fn shape_is_ephemeral_only(shape: &InterestShape) -> bool {
    !shape.kinds.is_empty() && shape.kinds.iter().all(|k| (20000..30000).contains(k))
}

/// In-place rewrite of every non-ephemeral sub-shape's `since` to
/// `max(existing_since, watermark + 1)`.
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
fn apply_watermark_rewrite(
    plan: &mut CompiledPlan,
    watermark_fn: &(dyn Fn(&InterestShape) -> Option<u64> + Send + Sync),
) {
    for relay_plan in plan.per_relay.values_mut() {
        for sub_shape in relay_plan.sub_shapes.iter_mut() {
            if shape_is_ephemeral_only(&sub_shape.shape) {
                continue;
            }
            let Some(watermark) = watermark_fn(&sub_shape.shape) else {
                continue;
            };
            let floor = watermark.saturating_add(1);
            sub_shape.shape.since = Some(match sub_shape.shape.since {
                Some(existing) if existing >= floor => existing,
                _ => floor,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::InMemoryMailboxCache;

    #[test]
    fn empty_lifecycle_starts_with_zero_compiles() {
        let l = SubscriptionLifecycle::new();
        assert_eq!(l.compile_count(), 0);
        assert!(l.current_plan.is_none());
    }

    #[test]
    fn empty_tick_does_not_compile() {
        let mut l = SubscriptionLifecycle::new();
        let mailboxes = InMemoryMailboxCache::new();
        let frames = l.drain_tick(&mailboxes);
        assert!(frames.is_empty());
        assert_eq!(l.compile_count(), 0);
    }
}
