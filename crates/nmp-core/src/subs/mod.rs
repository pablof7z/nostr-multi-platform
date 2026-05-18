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

use std::collections::BTreeSet;
use std::sync::Arc;

use auth_gate::AuthGate;
use lifecycle_gate::LifecycleGate;

use crate::planner::{
    apply_selection, CompiledPlan, InterestId, InterestShape, MailboxCache, PlannerError, RelayUrl,
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

/// Default upper bound on concurrent relay connections after greedy
/// max-coverage reduction. Mirrors the `outbox_perf` example budget.
pub const DEFAULT_SELECT_MAX_CONNECTIONS: usize = 30;

/// Default per-author redundancy cap (applesauce-pure). Each follow is
/// covered by at most this many surviving relays.
pub const DEFAULT_SELECT_MAX_PER_USER: usize = 2;

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
    /// Operator-configured app relays (T134).
    ///
    /// Threaded into the compiler on every recompile so author REQs ride
    /// the additive `UserConfigured(AppRelay)` lane on top of NIP-65 (or
    /// substitute when NIP-65 is unknown). Set via [`Self::set_app_relays`];
    /// defaults to empty so legacy lifecycle tests stay green.
    app_relays: Vec<RelayUrl>,
    /// Active account read relays — for no-author/no-address interests
    /// (hashtag firehose, global search). Set via
    /// [`Self::set_active_account_read_relays`]; defaults to empty so the
    /// no-author firehose falls back to `app_relays`, then indexer.
    active_account_read_relays: Vec<RelayUrl>,
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
    /// Greedy max-coverage budget — upper bound on concurrent relay
    /// connections after [`apply_selection`] reduces the naive plan.
    ///
    /// The naive M2 plan connects to every NIP-65 write relay declared by
    /// every follow (in real test data: 287 relays for 1048 follows). The
    /// selector reduces this to ~`select_max_connections` while preserving
    /// per-author coverage via [`Self::select_max_per_user`]. Default:
    /// [`DEFAULT_SELECT_MAX_CONNECTIONS`] (matches the `outbox_perf`
    /// example). Tune via [`Self::set_selection_budget`].
    select_max_connections: usize,
    /// Per-author redundancy cap — each follow may be served by at most
    /// this many surviving relays. Prevents the greedy algorithm from
    /// spending its whole connection budget on the popularity-distribution
    /// head while ignoring the long tail. Default:
    /// [`DEFAULT_SELECT_MAX_PER_USER`] (applesauce-pure).
    select_max_per_user: usize,
    /// Relays considered persistently unreachable. Filtered out of the plan
    /// BEFORE [`apply_selection`] runs, so the selector picks alternative
    /// NIP-65 write relays for the affected authors. Populated by the actor
    /// via [`Self::mark_relay_dead`] in response to repeated connect failures
    /// (heuristic owned by the caller — the lifecycle just respects the set).
    /// Cleared per-relay via [`Self::mark_relay_alive`] on a successful
    /// re-connection. Each transition fires [`CompileTrigger::RelayHealthChanged`]
    /// so the affected authors re-route on the next compile pass.
    dead_relays: BTreeSet<RelayUrl>,
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
            app_relays: Vec::new(),
            active_account_read_relays: Vec::new(),
            current_plan: None,
            lifecycle_gate: LifecycleGate::new(),
            auth_gate: AuthGate::new(),
            compile_count: 0,
            coverage_hook: None,
            watermark_fn: None,
            select_max_connections: DEFAULT_SELECT_MAX_CONNECTIONS,
            select_max_per_user: DEFAULT_SELECT_MAX_PER_USER,
            dead_relays: BTreeSet::new(),
        }
    }

    /// Mark a relay as persistently unreachable. The next recompile excludes
    /// it from the candidate set passed to [`apply_selection`], so authors
    /// who declared this relay route through their other NIP-65 write
    /// relays instead. Authors whose ENTIRE write set is dead fall off the
    /// plan (they cannot be reached) until a relay is marked alive.
    ///
    /// Returns true iff the relay's state changed (was alive, now dead).
    /// On change, enqueues [`CompileTrigger::RelayHealthChanged`].
    ///
    /// The actor owns the heuristic for what counts as "dead" — typically
    /// N consecutive connect failures within a window. This lifecycle just
    /// respects the actor's decision.
    pub fn mark_relay_dead(&mut self, url: RelayUrl) -> bool {
        let inserted = self.dead_relays.insert(url.clone());
        if inserted {
            self.inbox
                .enqueue(CompileTrigger::RelayHealthChanged { url, dead: true });
        }
        inserted
    }

    /// Clear a relay's dead mark. The next recompile lets the selector pick
    /// it again. Returns true iff the relay's state changed.
    pub fn mark_relay_alive(&mut self, url: &RelayUrl) -> bool {
        let removed = self.dead_relays.remove(url);
        if removed {
            self.inbox.enqueue(CompileTrigger::RelayHealthChanged {
                url: url.clone(),
                dead: false,
            });
        }
        removed
    }

    /// Read-only access to the dead-relay set (diagnostics).
    pub fn dead_relays(&self) -> &BTreeSet<RelayUrl> {
        &self.dead_relays
    }

    /// Install (or replace) the operator-configured app relay list (T134).
    ///
    /// The next recompile threads this list into the compiler so author
    /// REQs ride the additive `UserConfigured(AppRelay)` lane on top of
    /// (or in place of) NIP-65 write relays. Setting an empty list reverts
    /// to pure-NIP-65 routing; authors that subsequently lose their NIP-65
    /// mailbox land in `CompiledPlan::unroutable_authors`.
    pub fn set_app_relays(&mut self, relays: Vec<RelayUrl>) {
        self.app_relays = relays;
    }

    /// Install (or replace) the active-account read relay list (T134).
    ///
    /// Used by case_d (no-author firehose) as the primary routing target,
    /// unioned with `app_relays`. The kernel populates this from the active
    /// account's kind:10002 read-relays.
    pub fn set_active_account_read_relays(&mut self, relays: Vec<RelayUrl>) {
        self.active_account_read_relays = relays;
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
        let compiler = SubscriptionCompiler::with_relays(
            mailbox_cache,
            &self.indexer_relays,
            &self.active_account_read_relays,
            &self.app_relays,
        );
        let mut plan = compiler.compile(&interests)?;
        self.compile_count = self.compile_count.saturating_add(1);

        // Health filter: strip relays the actor has marked dead BEFORE the
        // selector runs. The selector's candidate set is then the alive
        // subset, so authors with a dead-only declared write set lose any
        // landing pad and the selector retires them into "uncovered" (they
        // simply don't appear in any surviving sub_shape). Authors with
        // mixed alive/dead declared write relays naturally pick the alive
        // ones during coverage rounds.
        //
        // Doing this BEFORE compile would shrink the plan_id input set;
        // doing it AFTER apply_selection would leave dead relays in the
        // wire diff. Between the two is the right seam.
        if !self.dead_relays.is_empty() {
            plan.per_relay.retain(|url, _| !self.dead_relays.contains(url));
        }

        // Greedy max-coverage selection — applesauce-style. The naive plan
        // connects to every NIP-65 write relay declared by every follow
        // (in real data: hundreds). This pass reduces the relay set to
        // ≤ `select_max_connections` with a per-author redundancy cap of
        // `select_max_per_user`. Runs BEFORE the coverage hook / watermark
        // so both downstream passes see only the surviving (relay, shape)
        // set. `apply_selection` mutates each affected `SubShape` in place
        // and calls `recompute_hash()` so the wire-emitter's diff produces
        // the correct REQ/CLOSE delta. Plan-id is intentionally NOT
        // recomputed (see `planner/mod.rs` §"Plan-id determinism vs.
        // post-compile mutators"; M4 precedent in
        // `docs/perf/codex-reviews/076173d.md`).
        apply_selection(&mut plan, self.select_max_connections, self.select_max_per_user);

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
    /// sub_id stability (`canonical_filter_hash` is computed off `shape` not
    /// the post-watermark filter — see `planner/mod.rs::canonical_filter_hash`
    /// rationale and the T129 carve-out in `apply_watermark_rewrite`).
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
                    if let Some(watermark) = wm(&wire_shape) {
                        let floor = watermark.saturating_add(1);
                        wire_shape.since = Some(match wire_shape.since {
                            Some(existing) if existing >= floor => existing,
                            _ => floor,
                        });
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

    /// T148 — test-only inspection of the AuthGate's per-URL pause predicate.
    /// Pins the per-URL keying invariant: a challenge that arrived on URL_B
    /// must NOT pause URL_A. See `kernel/auth_url_threading_tests.rs`.
    #[cfg(test)]
    pub(crate) fn is_auth_paused_for_url(&self, relay_url: &str) -> bool {
        self.auth_gate.is_paused(relay_url)
    }

    /// Install (or replace) the *discovery* indexer relay set used for
    /// kind:0 / kind:3 / kind:10002 lookups, event_id resolution, and the
    /// case-D cold-start fallback when both `app_relays` and the
    /// active-account read set are empty.
    ///
    /// Default at construction is `vec!["wss://purplepag.es".to_string()]`.
    /// Set to an empty `Vec` to disable indexer fallback entirely (authors
    /// without a mailbox snapshot will still land in
    /// `CompiledPlan::unroutable_authors` — case A never falls back to the
    /// indexer per T134's routing-rules clarification).
    ///
    /// Kernel-level only. FFI exposure is a separate API decision the user
    /// has not blessed yet — do NOT extend this through `crates/nmp-core/src/ffi`
    /// without that approval.
    pub fn set_indexer_relays(&mut self, relays: Vec<RelayUrl>) {
        self.indexer_relays = relays;
    }

    /// Override the greedy max-coverage selection budget used by the next
    /// recompile. Defaults: [`DEFAULT_SELECT_MAX_CONNECTIONS`] /
    /// [`DEFAULT_SELECT_MAX_PER_USER`].
    ///
    /// Setting `max_connections = 0` or `max_per_user = 0` drops every
    /// relay from the plan — almost certainly a config bug; callers are
    /// responsible for clamping if they ever expose this through
    /// configuration.
    pub fn set_selection_budget(&mut self, max_connections: usize, max_per_user: usize) {
        self.select_max_connections = max_connections;
        self.select_max_per_user = max_per_user;
    }

    /// Read-only access to the `indexer_relays` field — used by test
    /// scaffolds that verify `set_indexer_relays` mutated the field before
    /// continuing through a recompile.
    #[cfg(test)]
    pub(crate) fn indexer_relays(&self) -> &[RelayUrl] {
        &self.indexer_relays
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
    use crate::planner::{
        InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
        LogicalInterest, MailboxSnapshot,
    };

    fn pubkey(s: &str) -> String {
        format!("{s:0>64}").chars().take(64).collect()
    }

    /// Single-author follow interest (kind:1 timeline).
    fn follow(id: u64, author: &str) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: [pubkey(author)].into_iter().collect(),
                kinds: [1u32].into_iter().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
        }
    }

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

    // ─── apply_selection wiring ──────────────────────────────────────────────

    /// With 10 follows each declaring a unique write relay (no shared
    /// coverage), the naive plan would carry 10 relay entries. Bound
    /// `max_connections = 5` to force the greedy selector to actually prune
    /// — proving `apply_selection` is wired into `recompile_and_diff` (not a
    /// no-op).
    #[test]
    fn recompile_caps_per_relay_at_max_connections() {
        let mut l = SubscriptionLifecycle::new();
        l.set_app_relays(vec!["wss://app.example".to_string()]);
        // Tighten the budget so the test is independent of the default
        // (which would not prune at only 10 follows).
        let max_connections: usize = 5;
        l.set_selection_budget(max_connections, 2);

        let mut mailboxes = InMemoryMailboxCache::new();
        for i in 0..10u32 {
            let author_seed = format!("aa{i:02}");
            let relay = format!("wss://r{i:02}.example");
            mailboxes.put(
                pubkey(&author_seed),
                MailboxSnapshot {
                    write_relays: vec![relay],
                    read_relays: vec![],
                    both_relays: vec![],
                },
            );
            l.registry_mut().push(follow(u64::from(i) + 1, &author_seed));
        }

        let _frames = l.recompile_and_diff(&mailboxes).expect("compile");
        let plan = l.current_plan.as_ref().expect("plan present");
        assert!(
            plan.per_relay.len() <= max_connections,
            "per_relay.len() = {} must be ≤ max_connections = {}",
            plan.per_relay.len(),
            max_connections,
        );
    }

    /// A relay served by the naive plan on the first recompile drops out of
    /// the second when the selection budget is tightened. The wire-emitter
    /// diff MUST emit a CLOSE for every shape that was on the now-dropped
    /// relay (the diff iterates prior `per_relay` and CLOSEs any sub_id not
    /// in the next set — verifying that relays disappearing under selection
    /// are handled cleanly).
    #[test]
    fn dropped_relay_emits_close_on_next_recompile() {
        let mut l = SubscriptionLifecycle::new();
        // First compile with a generous budget — every relay survives.
        l.set_selection_budget(usize::MAX, usize::MAX);

        let mut mailboxes = InMemoryMailboxCache::new();
        for i in 0..3u32 {
            let author_seed = format!("bb{i:02}");
            let relay = format!("wss://drop{i:02}.example");
            mailboxes.put(
                pubkey(&author_seed),
                MailboxSnapshot {
                    write_relays: vec![relay],
                    read_relays: vec![],
                    both_relays: vec![],
                },
            );
            l.registry_mut().push(follow(u64::from(i) + 1, &author_seed));
        }

        let first = l.recompile_and_diff(&mailboxes).expect("first compile");
        let req_relays: std::collections::BTreeSet<String> = first
            .iter()
            .filter_map(|f| match f {
                WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            req_relays.len(),
            3,
            "first compile must REQ all 3 relays; got {req_relays:?}",
        );

        // Tighten the budget so 2 relays must be dropped on the next compile.
        l.set_selection_budget(1, 1);
        let second = l.recompile_and_diff(&mailboxes).expect("second compile");

        let plan = l.current_plan.as_ref().expect("plan present");
        assert_eq!(
            plan.per_relay.len(),
            1,
            "selection budget = 1 → exactly one relay survives; got {}",
            plan.per_relay.len(),
        );
        let surviving: std::collections::BTreeSet<String> =
            plan.per_relay.keys().cloned().collect();

        let closes: std::collections::BTreeSet<String> = second
            .iter()
            .filter_map(|f| match f {
                WireFrame::Close { relay_url, .. } => Some(relay_url.clone()),
                _ => None,
            })
            .collect();
        // Every relay that disappeared must have at least one CLOSE.
        let expected_dropped: std::collections::BTreeSet<String> =
            req_relays.difference(&surviving).cloned().collect();
        assert_eq!(expected_dropped.len(), 2, "two relays must have been dropped");
        for dropped in &expected_dropped {
            assert!(
                closes.contains(dropped),
                "wire-emitter diff must CLOSE the dropped relay {dropped}; got {closes:?}",
            );
        }
    }

    /// `set_indexer_relays` mutates the lifecycle's stored set and the next
    /// `recompile_and_diff` threads the override into the compiler.
    ///
    /// We do NOT assert via the resulting plan because the case-D cold-start
    /// path produces a wildcard-author sub-shape, which `apply_selection`
    /// (now wired into the recompile path) deliberately drops (see
    /// `selection.rs` §"Wildcard-author sub-shapes" — relays whose only
    /// contribution is wildcard coverage are dropped). Instead, this test
    /// (a) verifies the setter mutated the field, and (b) verifies the
    /// recompile path still consumes the field cleanly. The compile-time
    /// case-D cold-start behaviour is covered by
    /// `planner::compiler::partition::case_d_no_author::tests::case_d_cold_start_falls_through_to_indexer`.
    #[test]
    fn set_indexer_relays_is_reflected_in_next_recompile() {
        let mut l = SubscriptionLifecycle::new();
        assert_eq!(
            l.indexer_relays(),
            &["wss://purplepag.es".to_string()],
            "default indexer set is purplepag.es",
        );

        l.set_indexer_relays(vec!["wss://sentinel-indexer.example".to_string()]);
        assert_eq!(
            l.indexer_relays(),
            &["wss://sentinel-indexer.example".to_string()],
            "setter must replace the indexer set",
        );

        // Recompile with an empty registry should succeed (no-op compile)
        // and increment the compile counter — proving the new indexer set
        // is not poison input to the recompile path.
        let mailboxes = InMemoryMailboxCache::new();
        let prior = l.compile_count();
        let _ = l.recompile_and_diff(&mailboxes).expect("compile");
        assert_eq!(
            l.compile_count(),
            prior + 1,
            "recompile must run with the new indexer set installed",
        );
        // And the value must still be the override (not reset by recompile).
        assert_eq!(
            l.indexer_relays(),
            &["wss://sentinel-indexer.example".to_string()],
        );
    }

    // ─── dead-relay exclusion ────────────────────────────────────────────────

    /// An author who declares two write relays should land on the alive one
    /// when the other is marked dead. The dead relay must not appear in the
    /// resulting plan; the alive one must.
    #[test]
    fn dead_relay_excluded_from_next_recompile() {
        let mut l = SubscriptionLifecycle::new();
        l.set_selection_budget(usize::MAX, usize::MAX);

        let mut mailboxes = InMemoryMailboxCache::new();
        mailboxes.put(
            pubkey("cc01"),
            MailboxSnapshot {
                write_relays: vec![
                    "wss://alive.example".to_string(),
                    "wss://dead.example".to_string(),
                ],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        l.registry_mut().push(follow(1, "cc01"));

        // First compile: both relays present.
        let _ = l.recompile_and_diff(&mailboxes).expect("first compile");
        let before = l.current_plan.as_ref().expect("plan").per_relay.clone();
        assert!(before.contains_key("wss://alive.example"));
        assert!(before.contains_key("wss://dead.example"));

        // Mark dead.example as dead and recompile.
        assert!(l.mark_relay_dead("wss://dead.example".to_string()));
        let _ = l.recompile_and_diff(&mailboxes).expect("second compile");
        let after = &l.current_plan.as_ref().expect("plan").per_relay;
        assert!(
            after.contains_key("wss://alive.example"),
            "alive relay must still serve cc01"
        );
        assert!(
            !after.contains_key("wss://dead.example"),
            "dead relay must not appear in the plan"
        );
    }

    /// An author whose ENTIRE declared write set is dead falls out of the
    /// plan entirely (no candidate relay to route to). When a relay becomes
    /// alive again, the next recompile routes the author back to it.
    #[test]
    fn fully_dead_author_returns_when_relay_alive_again() {
        let mut l = SubscriptionLifecycle::new();
        l.set_selection_budget(usize::MAX, usize::MAX);

        let mut mailboxes = InMemoryMailboxCache::new();
        mailboxes.put(
            pubkey("dd01"),
            MailboxSnapshot {
                write_relays: vec!["wss://only.example".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        l.registry_mut().push(follow(1, "dd01"));

        // Compile, kill, recompile.
        let _ = l.recompile_and_diff(&mailboxes).expect("compile 1");
        assert!(l
            .current_plan
            .as_ref()
            .unwrap()
            .per_relay
            .contains_key("wss://only.example"));

        l.mark_relay_dead("wss://only.example".to_string());
        let _ = l.recompile_and_diff(&mailboxes).expect("compile 2");
        assert!(
            l.current_plan.as_ref().unwrap().per_relay.is_empty(),
            "all relays dead → empty plan"
        );

        // Resurrect.
        assert!(l.mark_relay_alive(&"wss://only.example".to_string()));
        let _ = l.recompile_and_diff(&mailboxes).expect("compile 3");
        assert!(l
            .current_plan
            .as_ref()
            .unwrap()
            .per_relay
            .contains_key("wss://only.example"));
    }

    /// Toggling a relay's state fires the `RelayHealthChanged` trigger.
    /// Marking an already-dead relay dead (or already-alive alive) is a no-op
    /// and does NOT enqueue a redundant trigger.
    #[test]
    fn mark_dead_idempotent_and_fires_trigger_only_on_change() {
        let mut l = SubscriptionLifecycle::new();
        assert!(l.mark_relay_dead("wss://x.example".to_string()));
        assert!(!l.mark_relay_dead("wss://x.example".to_string())); // already dead
        assert!(l.mark_relay_alive(&"wss://x.example".to_string()));
        assert!(!l.mark_relay_alive(&"wss://x.example".to_string())); // already alive
        assert!(l.dead_relays().is_empty());
    }
}
