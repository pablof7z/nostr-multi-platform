//! The keyed live read-collection primitive — shape (b) of #3115: a
//! Trellis-backed reconciled fan-out where a changing key-set each mounts an
//! INDEPENDENT live resource (its own output/lifecycle), as opposed to
//! [`crate::demand_set`]'s shape (a) (one shared reducer/output across every
//! member — demand_set already realizes shape (a) on the same
//! [`KeyedReconciler`] core, per #3116; nothing about it changes here).
//!
//! # Shape (b): N independent per-key live resources
//!
//! [`KeyedReadCollection<K, C>`] is the shape 29er's group-tree needs twice
//! over one key-set (#3115 "Consumer-driven requirements"): a per-group
//! observed feed keyed by `group_id`, and — in a SEPARATE collection
//! instance — a per-group presence session, also keyed by `group_id`. Each
//! key's member is a fully independent live resource with its own
//! output/lifecycle, not a row folding into one shared reducer.
//!
//! # Generic over host open/close (do NOT hardcode read-session)
//!
//! The per-key member the collection mounts may be a full
//! [`crate::open_read`]/[`crate::close_read`] read-session, a raw
//! `open_observed_projection`/`close_observed_projection` call, or any other
//! host-owned resource — 29er uses both across its two collections. This
//! type owns WHEN to open/close (the Trellis diff + apply-in-`Vec`-order);
//! the host owns WHAT opening means, supplied as a plain closure
//! ([`KeyedCollectionOpen`]) at construction. See `keyed_collection_tests.rs`
//! for a worked example of each flavor (a bare-closure "observed projection"
//! stand-in, and a real [`crate::ReadHost`]-backed read-session per key).
//!
//! # ResourceKey rule (load-bearing — collision-proof key_fn)
//!
//! Every [`KeyedReadCollection`] instance owns a PRIVATE
//! [`trellis_core::Graph`] (via its own [`KeyedReconciler`]) — never shared
//! across collections. This makes the #3115 "per-parent ownership, no
//! cross-collection coalescing" v1 default structural rather than a
//! convention any caller could violate: two different `KeyedReadCollection`
//! instances cannot coalesce or conflict, full stop, because their
//! `ResourceKey` namespaces are two different Trellis graphs.
//!
//! The remaining collision risk is entirely WITHIN one instance: `key_fn`
//! MUST be injective over `K` (two distinct `K` values must never derive the
//! same [`ResourceKey`]). Because the desired set a caller feeds
//! [`Self::reconcile`] is a `BTreeMap<K, C>`, two different `C` payloads can
//! never share one `K` in a single call — so an under-specified `key_fn` is
//! the ONLY way to trigger trellis-core's `GraphError::ResourcePayloadConflict`
//! hard-abort (a same-`ResourceKey` join with a divergent payload,
//! `resource_reconcile.rs:102-148` in trellis-core) or a silent same-key
//! coalesce (equal payload). In practice this means: derive `ResourceKey`
//! from the FULL identity `K` already carries (e.g. `group_id` plus whatever
//! else distinguishes two members within this one collection) — never a
//! truncated or partial view of `K`.
//!
//! # Exogenous-scalar dependency → `Replace`, not force-close+reopen
//!
//! A per-key descriptor may depend on a value that is NOT part of the
//! key-set itself (#3115 design note 3 — 29er's presence collection depends
//! on `active_pubkey`, which is no key's own identity). No separate
//! "descriptor-version" API is needed: `C: PartialEq` is already
//! [`KeyedReconciler`]'s own payload-identity contract, and trellis-core's
//! map diff (`MapDiff::between`, compares `V` via `PartialEq`) already
//! classifies a same-key value change as `updated` → `Replace`. The caller
//! embeds the exogenous scalar directly in `C` (e.g. a presence descriptor
//! carries `active_pubkey`) and re-supplies the SAME `K` with the new `C` on
//! the next [`Self::reconcile`] call — Trellis detects the divergence and
//! this type withdraws + remounts exactly that key, never the whole
//! collection. This replaces 29er's current "force-close+reopen every row on
//! identity change" with a real diff.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use nmp_core::trellis_reconciler::KeyedReconciler;
use trellis_core::{ResourceCommand, ResourceKey};

use crate::registry::TeardownAction;

/// Opaque per-member identity a [`KeyedReadCollection`] derives from `K` (via
/// its caller-supplied `key_fn`) and later hands back to the host `open`
/// closure. An NMP-owned wrapper around `trellis_core::ResourceKey`, never
/// the raw Trellis type itself: raw Trellis vocabulary stays confined to
/// `nmp-core`/`nmp-read-session` internals (#2858 Phase A ratchet) — a
/// public app/native/web-facing crate (e.g. an FFI-facade constructor) must
/// never need to import `trellis_core` just to write a `key_fn`/`open`
/// closure. See the module docs' ResourceKey rule for the injectivity
/// contract `key_fn` must uphold.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MemberKey(ResourceKey);

impl MemberKey {
    /// Builds a single-segment member key from deterministic, host-chosen
    /// identity (e.g. a group id, a relay URL).
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self(ResourceKey::new(key.into()))
    }

    /// The identity string this key was built from via [`Self::new`].
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Mounts one key's independent live resource and returns its teardown.
///
/// Called with the derived [`MemberKey`] (diagnostic identity only — the
/// closure never needs to recover the original `K` from it; `C` must be
/// self-describing, carrying whatever domain identity the mount needs, e.g.
/// a `group_id` field) and the OWNED `C` payload this key committed. Must do
/// whatever mounting `C` requires — open a read-session, open an observed
/// projection, anything host-owned — and return a [`TeardownAction`] that
/// undoes exactly that and nothing else.
///
/// Runs with NO lock this type owns held (see
/// `host_open_closure_can_call_back_into_the_collection_without_deadlocking`
/// in `keyed_collection_tests.rs`) — the necessary condition for composing
/// this primitive into an actor-lane executor, though not sufficient on its
/// own: the CALLER must still never invoke [`KeyedReadCollection::reconcile`]
/// or [`KeyedReadCollection::close`] from inside a snapshot/render-tick
/// closure (the #60 deadlock class, #3078-#3081) — call them on the
/// read/actor lane, once, at discovery-open/teardown time.
pub type KeyedCollectionOpen<C> = Arc<dyn Fn(&MemberKey, C) -> TeardownAction + Send + Sync>;

/// A keyed live read-collection: N independent per-key live resources whose
/// membership tracks a caller-fed desired set (#3115 shape (b)). See the
/// module docs for the ResourceKey rule and the exogenous-scalar pattern.
pub struct KeyedReadCollection<K, C>
where
    K: Clone + Ord + Send + Sync + 'static,
    C: Clone + PartialEq + Send + Sync + 'static,
{
    reconciler: KeyedReconciler<K, C>,
    live: Mutex<HashMap<String, TeardownAction>>,
    open: KeyedCollectionOpen<C>,
}

impl<K, C> KeyedReadCollection<K, C>
where
    K: Clone + Ord + Send + Sync + 'static,
    C: Clone + PartialEq + Send + Sync + 'static,
{
    /// Builds a fresh collection with an empty desired set.
    ///
    /// `scope_debug_name` is a Trellis-internal diagnostic label only, never
    /// surfaced to a concept. `key_fn` derives each member's [`MemberKey`]
    /// — see the module docs' ResourceKey rule for why it must be injective
    /// over `K`. `open` is the host-supplied mount/unmount applier — see
    /// [`KeyedCollectionOpen`].
    ///
    /// Infallible: [`MemberKey`] exists precisely so a `KeyedReadCollection`
    /// consumer never needs to import `trellis_core` vocabulary (#3129), and
    /// the underlying [`KeyedReconciler::new`] call below can only fail on a
    /// Trellis-internal graph-build error that is unreachable for a fresh,
    /// single-sequence construction over a brand-new empty graph — the same
    /// invariant [`crate::demand_set::open_read_demand_set`] already
    /// documents at its own `KeyedReconciler::new` call site.
    ///
    /// # Panics
    ///
    /// Never in practice — see above.
    #[must_use]
    pub fn new(
        scope_debug_name: impl Into<String>,
        key_fn: impl Fn(&K) -> MemberKey + Send + Sync + 'static,
        open: impl Fn(&MemberKey, C) -> TeardownAction + Send + Sync + 'static,
    ) -> Self {
        Self {
            reconciler: KeyedReconciler::new(scope_debug_name, move |k| key_fn(k).0)
                .expect("fresh KeyedReconciler construction over an empty graph cannot fail"),
            live: Mutex::new(HashMap::new()),
            open: Arc::new(open),
        }
    }

    /// Reconciles the live member set to exactly `desired`: mounts every
    /// newly-desired key, withdraws-then-remounts a key whose descriptor
    /// changed under an unchanged key (Trellis `Replace` — the
    /// exogenous-scalar pattern, module docs), and withdraws every key no
    /// longer desired. Applies the resource plan in `Vec` order — never
    /// sorted or parallelized, LIFO close correctness lives in that order —
    /// with the reconciler's own lock already released before any host
    /// `open`/teardown closure runs ([`KeyedReconciler::reconcile`] computes
    /// and returns the plan under its lock, dropping it on return, strictly
    /// before this method calls [`Self::apply`]).
    pub fn reconcile(&self, desired: BTreeMap<K, C>) {
        let commands = self.reconciler.reconcile(desired);
        self.apply(commands);
    }

    /// Closes the collection: the FINAL teardown plan, withdrawing every
    /// still-live member exactly once in reverse-acquisition (LIFO) order
    /// (substrate-guaranteed by trellis-core's scope-close ordering).
    /// Idempotent: a second call after close is a no-op, never a panic (D6).
    pub fn close(&self) {
        let commands = self.reconciler.close();
        self.apply(commands);
    }

    /// Runs Trellis's own `FullRecomputeCheck` oracle against this
    /// collection's graph — the cross-session leak-audit oracle every
    /// migrated reconciler wires into its equivalence harness (#3115/#3116):
    /// any owner-set divergence between the incremental path and a full
    /// recompute from canonical inputs is a leak, by construction.
    #[must_use]
    pub fn full_recompute_matches(&self) -> bool {
        self.reconciler.full_recompute_matches()
    }

    /// Count of currently-live members (diagnostic / leak audit).
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.live.lock().map(|live| live.len()).unwrap_or(0)
    }

    /// Applies a Trellis resource plan **in `Vec` order** — never sort or
    /// parallelize; LIFO close correctness on scope teardown lives in this
    /// order (mirrors `demand_set::apply_demand_set_commands`).
    fn apply(&self, commands: Vec<ResourceCommand<C>>) {
        for command in commands {
            match command {
                ResourceCommand::Open { key, command, .. } => {
                    self.open_one(MemberKey(key), command)
                }
                ResourceCommand::Replace { key, command, .. } => {
                    // No in-place "replace a live resource" primitive at any
                    // host seam this type composes over (same reason
                    // `demand_set::apply_demand_set_commands` withdraws +
                    // reopens): withdraw then remount under the SAME key.
                    // This IS the exogenous-scalar mechanism (module docs) —
                    // the caller re-supplies a changed `C` for a live key on
                    // the next `reconcile`, Trellis detects the `PartialEq`
                    // divergence and emits `Replace` instead of the caller
                    // hand-rolling a whole-collection force-close+reopen.
                    self.withdraw(key.as_str());
                    self.open_one(MemberKey(key), command);
                }
                ResourceCommand::Close { key, .. } => self.withdraw(key.as_str()),
                ResourceCommand::Refresh { .. } => {
                    // Never emitted by `KeyedReconciler`'s planner (only
                    // opens added / replaces updated / closes removed) —
                    // exhaustive match, not a reachable branch.
                }
            }
        }
    }

    fn open_one(&self, key: MemberKey, command: C) {
        let teardown = (self.open)(&key, command);
        if let Ok(mut live) = self.live.lock() {
            live.insert(key.as_str().to_string(), teardown);
        }
    }

    fn withdraw(&self, key: &str) {
        let action = self.live.lock().ok().and_then(|mut live| live.remove(key));
        if let Some(action) = action {
            action();
        }
    }
}

#[cfg(test)]
#[path = "keyed_collection_tests.rs"]
mod tests;
