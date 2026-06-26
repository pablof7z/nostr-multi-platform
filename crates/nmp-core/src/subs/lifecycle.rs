//! Construction + simple accessors/setters for [`SubscriptionLifecycle`].
//!
//! Split out of `subs/mod.rs` (file-size-gate, NMP #169) with zero
//! behavioural change. The struct definition itself stays in the module root
//! (it owns the privacy boundary); this sibling child module supplies the
//! constructor, the `Default` impl, and the field accessors/setters.

use std::collections::BTreeSet;

use crate::planner::RelayUrl;

use super::auth_gate::AuthGate;
use super::inbox::TriggerInbox;
use super::trigger::CompileTrigger;
use super::{
    InterestRegistry, PlanCoverageHook, SubscriptionLifecycle, WatermarkFn,
    DEFAULT_SELECT_MAX_CONNECTIONS, DEFAULT_SELECT_MAX_PER_USER,
};

impl Default for SubscriptionLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl SubscriptionLifecycle {
    /// Construct an empty lifecycle with no production indexer set.
    ///
    /// T132: the lifecycle no longer owns a mailbox cache. The caller passes a
    /// `&dyn MailboxCache` into `recompile_and_diff` / `drain_tick`, sourced
    /// from the kernel's `author_relay_lists` (via `KernelMailboxes`) in
    /// production, or an `InMemoryMailboxCache` constructed inline in tests.
    /// This eliminates the dual source-of-truth seam the planner-side cache
    /// previously created (T105 made `Kernel::author_relay_lists` authoritative).
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: InterestRegistry::new(),
            inbox: TriggerInbox::new(),
            indexer_relays: {
                #[cfg(test)]
                {
                    vec!["wss://indexer-relay.example".to_string()]
                }
                #[cfg(not(test))]
                {
                    Vec::new()
                }
            },
            app_relays: Vec::new(),
            active_account_read_relays: Vec::new(),
            bootstrap_content_relays: Vec::new(),
            bootstrap_indexer_relays: Vec::new(),
            current_plan: None,
            current_plan_attribution: std::collections::BTreeMap::new(),
            auth_gate: AuthGate::new(),
            compile_count: 0,
            coverage_hook: None,
            req_frame_interceptor: None,
            watermark_fn: None,
            select_max_connections: DEFAULT_SELECT_MAX_CONNECTIONS,
            select_max_per_user: DEFAULT_SELECT_MAX_PER_USER,
            dead_relays: BTreeSet::new(),
            probed_mailboxes: BTreeSet::new(),
            probe_epoch: 0,
            // Start `false` (NOT down) so the first indexer connect is treated
            // as an `up → up` no-op, not an outage recovery. The cold-start
            // probe is driven by the normal first recompile; the epoch must
            // count only genuine outages survived. The lane flips to `true`
            // only after `note_indexer_lane_recovered(false)` observes every
            // indexer socket gone, and the next recovery bumps the epoch.
            indexer_lane_down: false,
            last_planner_error: None,
            mailbox_generation: 0,
            watermark_generation: 0,
            last_compile_fingerprint: None,
        }
    }

    /// T140 (D6) — the most recent genuine planner error surfaced by
    /// [`Self::drain_tick`], or `None` if none has occurred. Benign
    /// `EmptyInterestSet` is never recorded here. Read by diagnostics / tests.
    #[must_use]
    pub fn last_planner_error(&self) -> Option<&str> {
        self.last_planner_error.as_deref()
    }

    /// Clear the implicit-discovery probed set so the next recompile
    /// re-probes every still-unknown author's kind:10002. The `refresh`
    /// escape hatch — e.g. after the indexer set changes or the operator
    /// wants to retry authors whose mailbox never arrived.
    ///
    /// Also clears the compile-input fingerprint so the memo guard in
    /// `recompile_and_diff_with_lookup` does not skip the next compile —
    /// the probed set is consumed by the post-compile probe-REQ emission
    /// path and must be visible to the next compile even when no other
    /// input changed.
    pub fn clear_probed_mailboxes(&mut self) {
        self.probed_mailboxes.clear();
        self.last_compile_fingerprint = None;
    }

    /// B3 — observe an indexer-lane connectivity edge and, on a genuine outage
    /// recovery, re-arm the mailbox-probe set.
    ///
    /// `any_indexer_connected` is the kernel's current truth: is *any* indexer
    /// socket connected right now? The kernel computes it from its per-URL
    /// transport rows (`relay_transport.rs`) on every indexer connect / fail /
    /// close and calls this method with the result.
    ///
    /// State machine (the edge gate that avoids the reverted per-reconnect churn):
    /// - `down → up` (`indexer_lane_down == true && any_indexer_connected`):
    ///   a genuine recovery. Bump [`Self::probe_epoch`], clear
    ///   `probed_mailboxes`, and enqueue an [`CompileTrigger::IndexerSetChanged`]
    ///   carrying the new epoch so the next `drain_tick` re-probes every
    ///   still-unknown author. Returns `true`.
    /// - `up → down` (`!any_indexer_connected`): record the outage edge so the
    ///   NEXT recovery is detected. No re-arm, no trigger. Returns `false`.
    /// - `up → up` (a reconnect while a sibling indexer stayed live, or the
    ///   cold-start first connect): no-op. This is exactly the routine-churn
    ///   case the naive re-arm regressed on — the epoch is stable and the live
    ///   probe set is untouched. Returns `false`.
    ///
    /// `indexer_lane_down` starts `false`, so the cold-start first connect is an
    /// `up → up` no-op and does NOT bump the epoch (the cold-start probe is the
    /// normal first recompile's job). The epoch therefore counts exactly the
    /// number of genuine indexer outages survived.
    ///
    /// D8: enqueues at most one trigger per genuine edge; no polling. D4: the
    /// lifecycle remains the single owner of `probed_mailboxes` / `probe_epoch`.
    pub fn note_indexer_lane_recovered(&mut self, any_indexer_connected: bool) -> bool {
        if any_indexer_connected {
            let was_down = self.indexer_lane_down;
            self.indexer_lane_down = false;
            if was_down {
                // Genuine 0→1 recovery (or cold-start first connect): re-arm.
                self.probe_epoch = self.probe_epoch.saturating_add(1);
                self.clear_probed_mailboxes();
                self.inbox.enqueue(CompileTrigger::IndexerSetChanged {
                    generation: self.probe_epoch,
                });
                return true;
            }
            false
        } else {
            // Lane went fully down — record the outage edge. No re-arm here:
            // re-probing while no indexer is reachable would only land
            // unroutable frames; the re-arm fires on the recovery edge.
            self.indexer_lane_down = true;
            false
        }
    }

    /// B3 — current mailbox-probe epoch (diagnostics / tests). Advances by one
    /// on each genuine indexer-lane outage recovery
    /// (see [`Self::note_indexer_lane_recovered`]).
    #[must_use]
    pub fn probe_epoch(&self) -> u64 {
        self.probe_epoch
    }

    /// Read-only view of the probed set (diagnostics / tests).
    #[must_use]
    pub fn probed_mailboxes(&self) -> &BTreeSet<String> {
        &self.probed_mailboxes
    }

    /// Mark a relay as persistently unreachable. The next recompile excludes
    /// it from the candidate set passed to [`crate::planner::apply_selection`],
    /// so authors who declared this relay route through their other NIP-65
    /// write relays instead. Authors whose ENTIRE write set is dead fall off
    /// the plan (they cannot be reached) until a relay is marked alive.
    ///
    /// Returns true iff the relay's state changed (was alive, now dead).
    /// On change, enqueues [`CompileTrigger::RelayHealthChanged`].
    ///
    /// The actor owns the heuristic for what counts as "dead" — typically
    /// N consecutive connect failures within a window. This lifecycle just
    /// respects the actor's decision.
    #[must_use]
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
    #[must_use]
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
    #[must_use]
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
    /// Used by `case_d` (no-author firehose) as the primary routing target,
    /// unioned with `app_relays`. The kernel populates this from the active
    /// account's kind:10002 read-relays.
    pub fn set_active_account_read_relays(&mut self, relays: Vec<RelayUrl>) {
        self.active_account_read_relays = relays;
    }

    /// PD-033-C — install (or replace) the cold-start bootstrap content relay
    /// list.
    ///
    /// Populated by the kernel from
    /// `bootstrap_urls_for_role(RelayRole::Content)`; threaded into the compiler
    /// on every recompile. Empty by default so existing call sites see no
    /// behavioural change; a `OneShot + Global + event_ids`-shaped discovery
    /// interest with an empty bootstrap set falls through to the unchanged
    /// Case D body. See
    /// [`crate::planner::compiler::SubscriptionCompiler::with_relays_and_bootstrap`]
    /// and `docs/retired/pd033c-routing-gaps.md` §4.3 for the routing
    /// rationale.
    pub fn set_bootstrap_content_relays(&mut self, relays: Vec<RelayUrl>) {
        self.bootstrap_content_relays = relays;
    }

    /// PD-033-C — install (or replace) the cold-start bootstrap indexer relay
    /// list.
    ///
    /// Populated by the kernel from
    /// `bootstrap_urls_for_role(RelayRole::Indexer)` — the WITH-FALLBACK form,
    /// including `FALLBACK_INDEXER_RELAY` when no indexer row is configured
    /// yet. Consumed by `case_a_authors::route`'s `OneShot + Global` discovery
    /// arm — distinct from [`Self::set_indexer_relays`] which feeds the raw
    /// (no-fallback) indexer probe / Case D cold-start fallback paths.
    ///
    /// Empty by default so existing call sites see no behavioural change. The
    /// kernel always sets this in `identity_state::set_configured_relays`.
    pub fn set_bootstrap_indexer_relays(&mut self, relays: Vec<RelayUrl>) {
        self.bootstrap_indexer_relays = relays;
    }

    /// Install (or replace) the post-compile [`PlanCoverageHook`].
    ///
    /// The actor calls this once at startup with the shell's coverage-filter
    /// closure (e.g. `Arc::new(|plan| { apply_coverage_filter(plan, …); })`)
    /// — `nmp-core` itself never knows the hook's identity. The seam itself
    /// is covered by `subs::coverage_hook_tests`.
    pub fn set_coverage_hook(&mut self, hook: PlanCoverageHook) {
        self.coverage_hook = Some(hook);
    }

    /// Install (or replace) the outbound planner REQ interceptor.
    ///
    /// The actor invokes this hook after planner registration and before raw
    /// NIP-01 serialization. Returning `None` from the hook keeps the original
    /// REQ path.
    pub fn set_req_frame_interceptor(
        &mut self,
        interceptor: std::sync::Arc<dyn crate::substrate::ReqFrameInterceptor>,
    ) {
        self.req_frame_interceptor = Some(interceptor);
    }

    /// Clone the installed outbound REQ interceptor, if any.
    #[must_use]
    pub(crate) fn req_frame_interceptor(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::substrate::ReqFrameInterceptor>> {
        self.req_frame_interceptor
            .as_ref()
            .map(std::sync::Arc::clone)
    }

    /// Advance the mailbox-cache generation counter by one.
    ///
    /// Called from [`Self::enqueue_trigger`] when a successful NIP-65
    /// (kind:10002 / kind:10050) ingest queues the corresponding mailbox
    /// trigger. That keeps mailbox-cache invalidation attached to the
    /// subscription lifecycle trigger rather than spread across kernel ingest
    /// call sites.
    ///
    /// Background: `KernelMailboxes::generation()` always returns `0` (the
    /// substrate cache has no built-in write counter), so this lifecycle-owned
    /// counter is the canonical signal for "mailbox data changed, recompile
    /// needed even if other inputs are stable".
    fn bump_mailbox_generation(&mut self) {
        self.mailbox_generation = self.mailbox_generation.saturating_add(1);
        self.last_compile_fingerprint = None;
    }

    /// Advance the watermark generation counter by one.
    ///
    /// The kernel calls this after every `record_eose_coverage` /
    /// `record_neg_done_coverage` write so the compile-input fingerprint
    /// (see `recompile_and_diff_with_lookup`) captures coverage-ledger changes.
    /// Without this call, a compile triggered after an EOSE might be skipped by
    /// the memo guard even though `apply_watermark_rewrite` would now produce
    /// different `since` values — causing silent under-fetch.
    pub fn bump_watermark_generation(&mut self) {
        self.watermark_generation = self.watermark_generation.saturating_add(1);
        // Clear the cached fingerprint so the next `recompile_and_diff` re-hashes
        // and re-runs regardless of any other input being unchanged.
        self.last_compile_fingerprint = None;
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

    /// Evaluate the installed watermark function for `shape` — test-only.
    ///
    /// ADR-0045 §6: lets the cache-serve invariant test assert the
    /// load-bearing implication "watermark floors the shape ⇒ cache-serve
    /// covers the shape" against the REAL production watermark closure, not
    /// a re-derivation of its rules.
    ///
    /// K3 Stage D2: the watermark fn is now per-`(shape, relay)`. These
    /// presence-invariant tests exercise the flag-OFF path (the default), where
    /// the relay is ignored entirely, so a fixed sentinel relay is passed. A
    /// dedicated relay-aware accessor is used by the D2 ledger-read tests.
    #[cfg(test)]
    pub(crate) fn watermark_for_shape_for_test(
        &self,
        shape: &crate::planner::InterestShape,
    ) -> Option<u64> {
        self.watermark_for_shape_on_relay_for_test(shape, "wss://watermark-test.invalid")
    }

    /// Evaluate the installed watermark function for `(shape, relay)` — test-only.
    ///
    /// K3 Stage D2: the relay-aware form, used by the coverage-ledger read-swap
    /// tests to assert that the floor for a given `(filter_hash, relay)` reads
    /// the ledger (flag on, row present) vs the presence fallback.
    #[cfg(test)]
    pub(crate) fn watermark_for_shape_on_relay_for_test(
        &self,
        shape: &crate::planner::InterestShape,
        relay_url: &str,
    ) -> Option<u64> {
        self.watermark_fn.as_ref().and_then(|f| f(shape, relay_url))
    }

    /// Mutable access to the registry — view modules push interests through
    /// this in production; integration tests push directly.
    pub fn registry_mut(&mut self) -> &mut InterestRegistry {
        &mut self.registry
    }

    /// Read-only access to the registry. The hot ingest path
    /// (`Kernel::should_store_event`, ADR-0042 §5.1) iterates the active
    /// interests to admit events matching a generic `open_interest`.
    #[must_use]
    pub fn registry(&self) -> &InterestRegistry {
        &self.registry
    }

    /// Compile counter (one increment per planner invocation).
    #[must_use]
    pub fn compile_count(&self) -> u64 {
        self.compile_count
    }

    /// Enqueue a trigger. Coalesced with siblings until the next `drain_tick`.
    pub fn enqueue_trigger(&mut self, trigger: CompileTrigger) {
        if matches!(
            trigger,
            CompileTrigger::Nip65Arrived { .. } | CompileTrigger::DmRelayListChanged { .. }
        ) {
            self.bump_mailbox_generation();
        }
        self.inbox.enqueue(trigger);
    }

    /// Install (or replace) the *discovery* indexer relay set used for
    /// kind:0 / kind:3 / kind:10002 lookups, `event_id` resolution, and the
    /// case-D cold-start fallback when both `app_relays` and the
    /// active-account read set are empty.
    ///
    /// Default at construction is a `.example` fixture under `#[cfg(test)]`;
    /// empty in production so the app-supplied set is authoritative.
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

    /// Count of triggers queued in the coalescing inbox but not yet drained.
    ///
    /// Test seam for kernel ingest tests that assert a recompile was *requested*
    /// (a `CompileTrigger` enqueued) without driving a full `drain_tick`. The
    /// inbox field is private to `subs::inbox`; this exposes only its length so
    /// callers can assert "≥1 trigger pending" after an ingest path runs.
    #[cfg(test)]
    pub(crate) fn pending_trigger_count(&self) -> usize {
        self.inbox.len()
    }

    /// #171 test seam — force a `last_planner_error` so the
    /// `KernelUpdate`/FFI projection can be exercised without a constructible
    /// `PlannerError` path. `PlannerError` variants are presently defensive
    /// (never constructed on a real compiler path — `compile_with_context`
    /// always returns `Ok`); this setter injects the recorded-error state the
    /// `drain_tick` `Err(e)` arm would set, so the D6 projection is testable
    /// today and any future genuine construction path surfaces automatically.
    #[cfg(test)]
    pub(crate) fn set_planner_error_for_test(&mut self, error: impl Into<String>) {
        self.last_planner_error = Some(error.into());
    }
}
