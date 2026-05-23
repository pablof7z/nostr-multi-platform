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
    /// Construct an empty lifecycle with a default indexer set.
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
                { vec!["wss://purplepag.es".to_string()] }
                #[cfg(not(test))]
                { Vec::new() }
            },
            app_relays: Vec::new(),
            active_account_read_relays: Vec::new(),
            current_plan: None,
            auth_gate: AuthGate::new(),
            compile_count: 0,
            coverage_hook: None,
            watermark_fn: None,
            select_max_connections: DEFAULT_SELECT_MAX_CONNECTIONS,
            select_max_per_user: DEFAULT_SELECT_MAX_PER_USER,
            dead_relays: BTreeSet::new(),
            probed_mailboxes: BTreeSet::new(),
            last_planner_error: None,
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
    pub fn clear_probed_mailboxes(&mut self) {
        self.probed_mailboxes.clear();
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
    /// Used by case_d (no-author firehose) as the primary routing target,
    /// unioned with `app_relays`. The kernel populates this from the active
    /// account's kind:10002 read-relays.
    pub fn set_active_account_read_relays(&mut self, relays: Vec<RelayUrl>) {
        self.active_account_read_relays = relays;
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
    #[must_use] 
    pub fn compile_count(&self) -> u64 {
        self.compile_count
    }

    /// Enqueue a trigger. Coalesced with siblings until the next `drain_tick`.
    pub fn enqueue_trigger(&mut self, trigger: CompileTrigger) {
        self.inbox.enqueue(trigger);
    }

    /// Install (or replace) the *discovery* indexer relay set used for
    /// kind:0 / kind:3 / kind:10002 lookups, event_id resolution, and the
    /// case-D cold-start fallback when both `app_relays` and the
    /// active-account read set are empty.
    ///
    /// Default at construction is `vec!["wss://purplepag.es".to_string()]` under
    /// `#[cfg(test)]`; empty in production so the app-supplied set is authoritative.
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
