//! Reusable Trellis-backed keyed reconciler (#3115/#3116).
//!
//! Several NMP reconcilers share one exact shape: a caller-controlled,
//! unbounded set of independent per-key live resources (a relay's discovery
//! read, a followed author's profile-ref, a per-group presence session…)
//! that grows and shrinks over a session's lifetime, where opening a new key
//! must never touch an already-live key and closing a key must run its
//! withdrawal exactly once. `nmp-read-session`'s demand-set engine
//! ([`crate::kernel`]'s sibling crate) hand-rolled this diff itself before
//! #3116; [`KeyedReconciler`] is the shared resource-reconciliation core the
//! read-layer keyed reconcilers build on, so the diff/teardown/replay-oracle
//! mechanics are written and leak-audited once (owner directive on #3116:
//! "default = migrate every hand-rolled reconciler onto Trellis"). It is
//! resource-plan-only by design — `nmp-feed-session`'s adapter is a richer,
//! output-fused variant that stays off this core because it commits output
//! `Clear` frames and the resource plan atomically in one Trellis scope; see
//! the recorded exception in ADR-0075.
//!
//! # Shape
//!
//! A [`KeyedReconciler<K, C>`] owns a PERSISTENT `trellis_core::Graph<C>`
//! with one scope and one `BTreeMap<K, C>` input. Each
//! [`KeyedReconciler::reconcile`] call stages the caller's current desired
//! map as that input and commits: Trellis diffs it against the previously
//! committed map and returns an ORDERED `Vec<ResourceCommand<C>>` — `Open`
//! for a newly-desired key, `Replace` for a key whose payload changed,
//! `Close` for a key no longer desired. The caller (the host) applies these
//! commands **in Vec order** against its own real resources — order is
//! load-bearing (trellis-core's own scope-teardown ordering guarantee is
//! LIFO close, and that correctness lives in the plan's order) and must
//! never be sorted or parallelized.
//!
//! # What this core does NOT own
//!
//! It never touches a host resource itself — no interest, no observed
//! projection, no output. It is pure reconciliation: desired map in,
//! ordered resource plan out. The host applier (e.g.
//! `nmp-read-session::demand_set`) owns turning an `Open{key, command}`
//! into a real subscription and a `Close{key, ..}` into its teardown —
//! including keeping its OWN `ResourceKey → handle` map, since `Close`
//! carries no payload (Trellis's executor contract; a spike read of
//! `trellis-core::resource_reconcile` confirmed `remove_resource_owner`
//! never re-emits the payload it closed).
//!
//! # Payload identity (trellis-core's resource-reconcile rule)
//!
//! `C: Clone + PartialEq` is Trellis's own resource-payload constraint:
//! identity is the `ResourceKey` alone (derived by the caller-supplied
//! `key_fn`), and payload is compared but is NOT part of identity. A
//! same-key join with an EQUAL payload silently coalesces (refcounted —
//! harmless here, since one `KeyedReconciler` instance is never shared
//! across two independent desired sets); a same-key join with a DIVERGENT
//! payload aborts the WHOLE commit with `GraphError::ResourcePayloadConflict`
//! (copy-on-commit ⇒ no partial state). `key_fn` MUST therefore encode
//! every parameter that distinguishes one live member from another within
//! this reconciler's own key space — under-specifying it is a correctness
//! bug, not a perf detail.

use std::collections::BTreeMap;
use std::sync::Mutex;

use trellis_core::{
    DependencyList, Graph, GraphResult, InputNode, ResourceCommand, ResourceKey, ResourcePlan,
    ScopeId,
};

/// One reusable Trellis-backed keyed reconciler. See module docs.
pub struct KeyedReconciler<K, C>
where
    K: Clone + Ord + Send + Sync + 'static,
    C: Clone + PartialEq + Send + Sync + 'static,
{
    inner: Mutex<KeyedReconcilerInner<K, C>>,
}

struct KeyedReconcilerInner<K, C> {
    graph: Graph<C>,
    scope: ScopeId,
    demand_input: InputNode<BTreeMap<K, C>>,
    closed: bool,
}

impl<K, C> KeyedReconciler<K, C>
where
    K: Clone + Ord + Send + Sync + 'static,
    C: Clone + PartialEq + Send + Sync + 'static,
{
    /// Builds a fresh reconciler with an empty desired set.
    ///
    /// `scope_debug_name` is a Trellis-internal diagnostic label only, never
    /// surfaced to a concept. `key_fn` derives each member's `ResourceKey`
    /// from its `K` — see the payload-identity module docs for why it must
    /// be collision-proof within this reconciler's key space.
    pub fn new(
        scope_debug_name: impl Into<String>,
        key_fn: impl Fn(&K) -> ResourceKey + Send + Sync + 'static,
    ) -> GraphResult<Self> {
        let mut graph = Graph::<C>::new_with_command_type();
        let mut tx = graph.begin_transaction()?;
        let scope = tx.create_scope(scope_debug_name)?;
        let demand_input = tx.input::<BTreeMap<K, C>>("keyed-reconciler-demand")?;
        tx.set_input(demand_input, BTreeMap::new())?;
        tx.attach_node_to_scope(demand_input, scope)?;
        let demand = tx.map_collection(
            "keyed-reconciler-demand-map",
            DependencyList::new([demand_input.id()])?,
            move |ctx| Ok(ctx.input(demand_input)?.clone()),
        )?;
        tx.map_resource_planner(demand, scope, move |ctx| {
            let mut plan = ResourcePlan::new();
            for added in &ctx.diff().added {
                let (key, command) = &added.value;
                plan.open(key_fn(key), ctx.scope(), command.clone());
            }
            for updated in &ctx.diff().updated {
                plan.replace(key_fn(&updated.key), ctx.scope(), updated.current.clone());
            }
            for removed in &ctx.diff().removed {
                let (key, _) = &removed.value;
                plan.close(key_fn(key), ctx.scope());
            }
            Ok(plan)
        })?;
        tx.commit()?;
        drop(tx);
        Ok(Self {
            inner: Mutex::new(KeyedReconcilerInner {
                graph,
                scope,
                demand_input,
                closed: false,
            }),
        })
    }

    /// Reconciles the live member set to exactly `desired`, returning the
    /// ordered resource plan (apply in `Vec` order — see module docs).
    ///
    /// D6 — a poisoned lock or a failed commit degrades to an empty plan
    /// rather than a panic; the graph's last-committed state stays the
    /// source of truth for the next call.
    #[must_use]
    pub fn reconcile(&self, desired: BTreeMap<K, C>) -> Vec<ResourceCommand<C>> {
        let Ok(mut inner) = self.inner.lock() else {
            return Vec::new();
        };
        inner.reconcile(desired)
    }

    /// Closes the reconciler's scope — the FINAL teardown plan, closing
    /// every still-live member exactly once in reverse-acquisition (LIFO)
    /// order (substrate-guaranteed by trellis-core's scope-close ordering). Idempotent: a second
    /// call after close returns an empty plan, never a panic (D6).
    #[must_use]
    pub fn close(&self) -> Vec<ResourceCommand<C>> {
        let Ok(mut inner) = self.inner.lock() else {
            return Vec::new();
        };
        inner.close()
    }

    /// Runs Trellis's own `FullRecomputeCheck` oracle against this
    /// reconciler's graph and returns whether incremental state matches a
    /// full recompute from canonical inputs. The leak-audit oracle every
    /// migrated reconciler wires into its equivalence harness (#3115/#3116):
    /// any owner-set divergence between the incremental path (what
    /// `reconcile`/`close` actually produced) and a full recompute is a
    /// leak, by construction. A poisoned lock degrades to `false` (D6).
    #[must_use]
    pub fn full_recompute_matches(&self) -> bool {
        let Ok(inner) = self.inner.lock() else {
            return false;
        };
        inner.graph.full_recompute().is_ok()
    }
}

impl<K, C> KeyedReconcilerInner<K, C>
where
    K: Clone + Ord + Send + Sync + 'static,
    C: Clone + PartialEq + Send + Sync + 'static,
{
    fn reconcile(&mut self, desired: BTreeMap<K, C>) -> Vec<ResourceCommand<C>> {
        if self.closed {
            return Vec::new();
        }
        let Ok(mut tx) = self.graph.begin_transaction() else {
            return Vec::new();
        };
        if tx.set_input(self.demand_input, desired).is_err() {
            return Vec::new();
        }
        let Ok(result) = tx.commit() else {
            return Vec::new();
        };
        drop(tx);
        result.resource_plan.into_commands()
    }

    fn close(&mut self) -> Vec<ResourceCommand<C>> {
        if self.closed {
            return Vec::new();
        }
        self.closed = true;
        let Ok(mut tx) = self.graph.begin_transaction() else {
            return Vec::new();
        };
        if tx.close_scope(self.scope).is_err() {
            return Vec::new();
        }
        let Ok(result) = tx.commit() else {
            return Vec::new();
        };
        drop(tx);
        result.resource_plan.into_commands()
    }
}

#[cfg(test)]
#[path = "trellis_reconciler_tests.rs"]
mod tests;
