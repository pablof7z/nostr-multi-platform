# ADR-0078: Keyed live read-collection — the demand-reconciler family primitive

## Decision

NMP has a reusable shape for "a caller-controlled, unbounded set of
independent per-key live resources that grows and shrinks over a session's
lifetime, where opening a new key must never touch an already-live key and
closing a key must run its withdrawal exactly once." A reconciler in this
category — a relay's discovery read, a followed author's profile-ref, a
per-group presence session — can build on the SAME Trellis-backed core
instead of hand-rolling its own added/removed diff. Using the shared core is
a per-case engineering choice made on the merits, not a requirement (per
[ADR-0077](0077-doctrines-are-guardrails-not-dogma.md)): a hand-rolled keyed
diff is a legitimate option too when it's the better fit. Per
[ADR-0075](0075-trellis-private-reconciliation-substrate.md), when Trellis is
used it stays private reconciliation machinery below this primitive; nothing
here exposes Trellis vocabulary to app/native/web-facing callers.

```text
KeyedReconciler<K, C>      — nmp-core: the private core. Desired map in,
                              ordered ResourceCommand<C> plan out. Pure
                              reconciliation; touches no host resource.
KeyedReadCollection<K, C>  — nmp-read-session: the public primitive. Wraps
                              a KeyedReconciler and a host-supplied
                              open/close closure; applies the plan.
```

Two shapes share this core:

- **Shape (a) — one shared reducer/output.** `nmp_read_session::demand_set`
  (`open_read_demand_set`/`reconcile_read_demand_set`): every member folds
  into ONE typed output through one shared reducer. Right for "one group's
  events, sourced from N relays" — the member set changes, the output
  identity does not.
- **Shape (b) — N independent live resources.** `KeyedReadCollection<K, C>`:
  each key mounts its OWN fully independent resource (its own output, its own
  lifecycle) — a per-group last-message read, a per-group presence session.
  Members do not share a reducer or an output; they only share membership
  bookkeeping.

Both shapes are thin appliers over the same `KeyedReconciler<K, C>`: they
differ only in what "apply an `Open`/`Close`/`Replace` command" means, never
in how membership is diffed.

## The `KeyedReconciler<K, C>` core

`crates/nmp-core/src/trellis_reconciler.rs`. Owns a persistent
`trellis_core::Graph<C>` with one scope and one `BTreeMap<K, C>` input.

```rust
pub struct KeyedReconciler<K, C>
where
    K: Clone + Ord + Send + Sync + 'static,
    C: Clone + PartialEq + Send + Sync + 'static;

impl<K, C> KeyedReconciler<K, C> { /* bounds as above */
    pub fn new(
        scope_debug_name: impl Into<String>,
        key_fn: impl Fn(&K) -> ResourceKey + Send + Sync + 'static,
    ) -> GraphResult<Self>;

    #[must_use]
    pub fn reconcile(&self, desired: BTreeMap<K, C>) -> Vec<ResourceCommand<C>>;

    #[must_use]
    pub fn close(&self) -> Vec<ResourceCommand<C>>;

    #[must_use]
    pub fn full_recompute_matches(&self) -> bool;
}
```

`reconcile` stages the caller's current desired map as the graph's input and
commits: Trellis diffs it against the previously committed map and returns an
ORDERED `Vec<ResourceCommand<C>>` — `Open` for a newly-desired key, `Replace`
for a key whose payload changed under an unchanged key, `Close` for a key no
longer desired. `close` closes the reconciler's scope and returns the FINAL
teardown plan for every still-live member. A poisoned lock or a failed commit
degrades to an empty plan, never a panic (D6); the graph's last-committed
state stays the source of truth for the next call.

The caller applies the returned plan **in `Vec` order** against its own real
resources — order is load-bearing, never sort or parallelize it. Two ordering
axes, two different owners:

- **Close-vs-Close (LIFO reverse-of-open): substrate-guaranteed.** Within a
  closing scope, Close is emitted in descending acquisition order; across
  scopes, children-before-parents postorder. The host does not need to
  hand-impose reverse-of-open teardown.
- **Cross-lane order (interest-retraction vs. output-tombstone vs.
  revision): host-imposed.** `resource_plan` and `output_frames` arrive as
  two separate fields from the same Trellis commit; the substrate produces
  `resource_plan` before `output_frames` but does not sequence how the host
  applies them. **Adapter obligation:** drain `resource_plan` before
  `output_frames` wherever a `KeyedReconciler`/`KeyedReadCollection` composes
  with a typed-session executor — this realizes "interest retraction before
  output tombstone."

`full_recompute_matches` runs Trellis's own `FullRecomputeCheck` oracle: it
rebuilds ownership from canonical inputs and reports any divergence from the
incrementally-committed state. This is the leak-audit oracle every migrated
reconciler wires into its equivalence harness — a divergence is a leak, by
construction.

## The `ResourceKey` identity rule (load-bearing)

Trellis's resource-reconcile identity is the `ResourceKey` **alone**; the
payload `C` is compared but is NOT part of identity (`C: Clone + PartialEq`
is the substrate's own payload constraint). On a same-key join:

- **equal payload → silent refcounted coalesce.** One owner's resource serves
  both; the final `Close` fires only when the last owner releases.
- **divergent payload → hard `GraphError::ResourcePayloadConflict` that
  aborts the WHOLE commit** (copy-on-commit ⇒ no partial state).

So an under-specified `key_fn` is not merely a perf detail — it is either a
silent cross-member merge or a hard commit-abort, depending on whether the
colliding payloads happen to match. `key_fn` MUST be a total function of
every parameter that distinguishes one live member from another within the
reconciler's own key space: anything that changes the wire subscription —
filters, relay set, `since`/window, policy — belongs in the key, and
`feed_author_refs`'s composite `(consumer_id, author_key)` key
(`ResourceKey::from_segments`) is the worked example of encoding compound
identity rather than a delimited string.

**v1 default: per-parent ownership, no cross-collection coalescing.** Every
`KeyedReadCollection` instance owns a PRIVATE `trellis_core::Graph` (via its
own `KeyedReconciler`) — never shared across collections. This makes
per-parent ownership structural rather than a convention a caller could
violate: two different `KeyedReadCollection` instances cannot coalesce or
conflict, full stop, because their `ResourceKey` namespaces are two different
Trellis graphs. The remaining collision risk is entirely WITHIN one instance:
`key_fn` must be injective over `K`. Cross-collection shared child reads
(dropping parent identity + enforcing payload equality + registry
shared-owner support) is a deliberate later step, only once a global
read-sharing story is proven — not a v1 concern.

## Generic host open/close

Neither `KeyedReconciler` nor `KeyedReadCollection` names a read-session or
an observed projection. `KeyedReconciler` never touches a host resource at
all — pure reconciliation, desired map in, ordered plan out. `KeyedReadCollection`
takes a host-supplied open/close closure at construction:

```rust
pub type KeyedCollectionOpen<C> =
    Arc<dyn Fn(&MemberKey, C) -> TeardownAction + Send + Sync>;

impl<K, C> KeyedReadCollection<K, C> {
    pub fn new(
        scope_debug_name: impl Into<String>,
        key_fn: impl Fn(&K) -> MemberKey + Send + Sync + 'static,
        open: impl Fn(&MemberKey, C) -> TeardownAction + Send + Sync + 'static,
    ) -> trellis_core::GraphResult<Self>;

    pub fn reconcile(&self, desired: BTreeMap<K, C>);
    pub fn close(&self);
    #[must_use] pub fn full_recompute_matches(&self) -> bool;
    #[must_use] pub fn live_count(&self) -> usize;
}
```

Trellis (via `KeyedReconciler`) owns WHEN to open/close a member; the host
owns WHAT opening means. The per-key member may be a full
`nmp_read_session::open_read`/`close_read` read-session, a raw
`open_observed_projection`/`close_observed_projection` call, or any other
host-owned resource — 29er's group-tree uses both, in two separate
collection instances (one per collection, per the per-parent-ownership rule
above). `nmp-uniffi-support` supplies two ready-made constructors
(`crates/nmp-uniffi-support/src/keyed_read_collection.rs`) —
`keyed_read_session_collection` and `keyed_observed_projection_collection` —
that wire the repeated `NmpApp` open/close plumbing so a facade only supplies
its own `key_fn`/`spec_for`/`projection_for` closures. Neither constructor
crosses into raw Trellis vocabulary: both are typed entirely in
`nmp_read_session::MemberKey` (the NMP-owned wrapper around
`trellis_core::ResourceKey`), never the raw Trellis type — the ADR-0075
public-surface boundary applies here exactly as it does everywhere else.

## Exogenous-scalar dependency → `Replace`, not force-close+reopen

A per-key descriptor may depend on a value that is NOT part of the key-set
itself — 29er's presence collection depends on `active_pubkey`, which is no
key's own identity. No separate "descriptor-version" API exists or is
needed: `C: PartialEq` is already the payload-identity contract, and
Trellis's map diff already classifies a same-key value change as `updated` →
`Replace`. The caller embeds the exogenous scalar directly in `C` (e.g. a
presence descriptor carries `active_pubkey`) and re-supplies the SAME `K`
with the new `C` on the next `reconcile` call; Trellis detects the
`PartialEq` divergence and this primitive withdraws + remounts exactly that
key. This replaces the "force-close+reopen every row on identity change"
pattern 29er used before this primitive existed with a real diff — only the
key(s) whose descriptor actually changed are touched.

`KeyedReadCollection` has no in-place "replace a live resource" primitive at
any host seam it composes over (the same reason `demand_set`'s applier
withdraws + reopens on `Replace`), so applying a `Replace` command means
withdraw-then-remount under the same key — functionally correct, but the
host's teardown+mount pair should stay cheap if a descriptor is expected to
churn often.

## Drain-order adapter obligation (executor contract)

Any executor composing a `KeyedReconciler`/`KeyedReadCollection` with a
typed-session output pipeline (revision bump, output frame emission) must
drain the returned `resource_plan` before touching `output_frames` for the
same commit. This is not automatic — the substrate produces both artifacts
from one commit but does not sequence how a host applies them across the two
lanes. Getting this wrong means an output tombstone could be observed before
its interest retraction actually landed. `demand_set` and
`feed_author_refs`'s appliers already apply this rule (apply the
`Vec<ResourceCommand<C>>` in order, then mark the output changed); any new
adapter over this core must do the same.

## `FullRecomputeCheck` as leak oracle

Every migrated/new reconciler over this core should wire
`full_recompute_matches` (or the equivalent `Graph::full_recompute` call
directly) into its own equivalence/leak-audit harness: run the incremental
path, then assert a full recompute from canonical inputs agrees with the
committed owner set. Any divergence is a resource leak by construction — this
is the SAME oracle the feed-session Trellis adoption (ADR-0075) already
established as the harness pattern for promoting a Trellis-backed path to
authority.

## Lane discipline: never reconcile from a render/snapshot closure

`reconcile`/`close` must run on the read/actor lane, once, at
discovery-open/teardown time — never inside a snapshot-tick or render closure.
This is not a style preference: calling `.sync()`-shaped reconcile logic
inside a snapshot-tick closure that itself may re-enter the registry is
exactly the #60 deadlock class (#3078-#3081) — a closure running under a
registry lock that reopens a read session re-locks the same registry. The
fix is structural, not a lock-ordering patch: open the collection ONCE at
discovery-open time on the read/actor lane; the tick closure only READS the
collection's current per-key outputs. `KeyedReadCollection`'s host `open`
closure itself runs with NO lock this type owns held, which is a *necessary*
condition for composing it safely into an actor-lane executor — but it is
not *sufficient* on its own: the caller must still never invoke
`reconcile`/`close` from inside a render/snapshot closure.

## Context

#3116 audited every open-added/close-removed/drain-on-close reconciler in the
read layer against a hard default: migrate onto Trellis, or record a
structural (dependency-direction or bootstrap-ordering) reason not to. That
sweep resolved to two migrations (`demand_set`, `feed_author_refs`) and one
deletion (dead `replace_dependent_interest_set`) — see
[ADR-0075](0075-trellis-private-reconciliation-substrate.md)'s "Read-layer
reconciler adoption" section for the completed-migration record. Doing that
work surfaced a reusable core (`KeyedReconciler<K, C>`) general across
single-segment, string, and composite keys.

Independently, #3115 scouted 29er's group-tree (the flagship consumer) and
found it needed the SAME shape a third time, but as a PUBLIC primitive rather
than an internal reconciler: N independent per-key live resources (shape b),
generic over host open/close (not hard-wired to read-sessions), with an
exogenous-scalar dependency and the exact snapshot-tick-closure deadlock this
primitive exists to structurally rule out. Building `KeyedReadCollection` on
the SAME core `demand_set`/`feed_author_refs` had just been migrated onto
avoided writing a third bespoke diff/teardown/oracle implementation for what
is, underneath, the same reconciliation problem every time.

## Consequences

NMP has a reusable reconciliation core available for this problem family,
instead of every caller writing its own bespoke `HashSet`/`BTreeMap` diff with
its own teardown-ordering and leak-audit story. New per-key live-resource
needs (a new protocol module's per-group session, a new app's per-topic read)
can compose `KeyedReconciler` directly if they need a private reconciler, or
`KeyedReadCollection` / `nmp-uniffi-support`'s two facade constructors if they
need a public per-key-live-resource primitive — or hand-roll a diff, when
that's the better fit for the case at hand.

The cost is the same one recorded in ADR-0075's Phase-A ratchet: both
`nmp-core` and `nmp-read-session` (and therefore every consumer that links
either) carry `trellis-core`'s transitive compile weight.

## Boundaries

Permitted:

- private `KeyedReconciler<K, C>` instances inside substrate/kernel crates
  (`nmp-core`, `nmp-read-session`) for an internal open-added/close-removed
  reconciler;
- `KeyedReadCollection<K, C>` as the public shape-(b) primitive, composed via
  `nmp-uniffi-support`'s facade constructors or directly against any
  `nmp_read_session`-typed host;
- host open/close closures that mount a read-session, an observed projection,
  or any other host-owned resource;
- embedding an exogenous scalar directly in the payload `C` so a value change
  diffs to `Replace`;
- a hand-rolled open-added/close-removed diff for a new keyed-set reconciler,
  chosen on the merits case-by-case — Trellis is an available option, not a
  mandatory one (per [ADR-0077](0077-doctrines-are-guardrails-not-dogma.md)).

Forbidden:

- a `key_fn`/`MemberKey` derivation that omits any parameter that
  distinguishes one live member from another (the collision/coalesce/abort
  hazard above);
- calling `reconcile`/`close` from inside a snapshot/render-tick closure;
- exposing `trellis_core::ResourceKey`, `ResourceCommand`, or `Graph` in an
  app/native/web-facing surface — always the `MemberKey`/`TeardownAction`
  wrapper types, per ADR-0075;
- cross-collection key coalescing (sharing one `KeyedReadCollection` graph
  across two logically distinct collections) before a global read-sharing
  design is proven.

## Enforcement

`nmp-core/src/trellis_reconciler_tests.rs`,
`nmp-read-session/src/keyed_collection_tests.rs`, and
`nmp-uniffi-support/src/keyed_read_collection.rs`'s own tests cover: added
members open, removed members close, an unchanged member is left untouched,
a changed payload under an unchanged key emits `Replace` (withdraw+remount),
`close` tears down every live member exactly once, and
`full_recompute_matches` agrees with the incremental path. ADR-0075's
doctrine gates (no Trellis vocabulary in app/native/web-facing APIs or
builder docs) apply unchanged to this primitive's public surface.

## Related

- [ADR-0075](0075-trellis-private-reconciliation-substrate.md) - Trellis as
  private reconciliation substrate; the read-layer adoption record.
- [ADR-0070](0070-typed-read-sessions.md) - typed read sessions own
  app-visible read lifecycles.
- [Builder guide 28a](../builder-guide/28a-build-a-keyed-live-read-collection.md) -
  worked example.
- #3115 - keyed live read-collection primitive.
- #3116 - read-layer reconciler consolidation epic.
- #3121 - `demand_set` migration + reusable `KeyedReconciler` core.
- #3122 - `feed_author_refs` migration.
- #3123 - `KeyedReadCollection` primitive PR.
- #3078-#3081 - the snapshot-tick-closure deadlock class this primitive's
  lane discipline structurally rules out.
