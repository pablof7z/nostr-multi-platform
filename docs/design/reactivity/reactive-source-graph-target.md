# Reactive Source Graph Target

Status: WIP / draft target architecture plus initial scaffolding and first
consumer adapter. This is not an ADR and does not claim that existing ADRs
settle the matter. Existing ADRs, docs, and code are evidence of the
architecture that exists today; this file sketches the re-architecture target
for one first consumer.

Audience: framework contributors. App APIs must not expose generic
`Signal`/Rx/observable streams in this first step. The graph described here is
an internal Rust substrate that preserves the typed feed/session/projection
surface apps already use.

## Problem Statement

The first target consumer is:

```
active account
  -> latest kind:3/contact-list truth
  -> active follows
  -> feed source
  -> dependent acquisition interests
  -> app-owned feed projection
```

The shipped stack has most of these pieces, but the reactivity is spread across
several mechanisms:

- `ActiveAccountSlot` plus identity observers signal account changes.
- The kernel event store owns accepted kind:3 events; latest contact-list
  follows are read through `latest_kind3_follows_from_store`.
- `ActiveFollowSet` mirrors active follows and fires callbacks.
- feed-session reducers build `live_shape`, `extra_acquisition`, reset hooks,
  resolver observers, and dependent interests.
- `ReplaceDependentInterestSet` owns child interest withdrawal/upsert in the
  kernel.
- observed projections replay/cache-serve matching events into feed engines.
- snapshot/feed render sources emit typed app projections.

That split works, but it makes the dataflow hard to audit. A source change has
to be threaded manually through observer sync, dependent-interest replacement,
pull-shape refresh, feed reset, and typed projection emission. The target is an
internal graph where each fact has one writer and every derived fact declares
its dependency on upstream nodes.

## Current Architecture Seams

Relevant current evidence:

- `crates/nmp-core/src/slots.rs` defines the current latest-kind:3 store read
  seam. `latest_kind3_follows_from_store` distinguishes no kind:3 (`None`) from
  an explicit empty kind:3 (`Some(vec![])`).
- `crates/nmp-nip02/src/projection.rs` keeps `FollowListProjection` as a thin
  read model over the event store, with kind:3 acquisition opened by
  `register_follow_state_runtime`.
- `crates/nmp-nip02/src/active_follow_set.rs` now adapts the first source graph
  consumer: it derives active follows from active account plus latest kind:3
  follows, writes a predicate read cache from graph effects, and exposes the
  existing callbacks.
- `crates/nmp-native-runtime/src/op_feed_defaults/session_compile/resolve.rs`
  resolves `FeedScope::ActiveUserFollows` into `ActiveFollowSet`, resolver
  observed projection, live timeline shape, extra acquisition, reset hooks, and
  active-account identity observer.
- `crates/nmp-native-runtime/src/op_feed_defaults/session_compile/source.rs`
  defines `ReducedSource`, the current bundle of admission, attribution,
  acquisition children, live pull shape, reset hooks, and teardown ids.
- `crates/nmp-native-runtime/src/op_feed_defaults/session_compile/session_engine.rs`
  turns a `ReducedSource` into a feed session, including observer sync,
  dependent-interest replacement, pull controller, reset hooks, and typed
  projection registration.
- `crates/nmp-browser-runtime/src/feed.rs` has a separate home-feed compiler
  path. That duplication should be treated as authority risk, not as a second
  implementation model to preserve.
- `crates/nmp-note-feed/src/op_feed/wiring.rs` currently defines
  `OP_FEED_SNAPSHOT_KEY = "nmp.feed.home"`. The reusable framework fact is the
  OP feed schema/codec, not a framework-owned product projection key.
- `crates/nmp-core/src/kernel/dependent_interests.rs` already has the kernel
  primitive that replaces a complete source-owned child-interest set and
  withdraws disappeared children.
- `docs/builder-guide/07-subscription-planner.md` documents internal source
  reducers as `source interest -> deterministic reducer -> materialized child
  interests`; the graph target should make that a first-class internal model.
- `docs/design/reactivity/loop-and-reverse-index.md` and siblings already define
  Rust-internal reactivity for event-to-view wakeups. This document extends that
  idea to source-to-source invalidation.

## Source-Of-Truth Corrections Before The Graph

Do not build the graph on top of the current accidental authorities.

- The graph's canonical contact-list input must be latest accepted/stored kind:3
  plus any explicit reducer-owned local baseline. Production paths such as
  contact prepopulation or follow-edit fallback must not introduce a second
  contact-cache authority for follow truth.
- The product feed projection key must be app/session-owned. NMP can own
  `nmp.note_feed.opfeed`, the FlatBuffer file identifier, codecs, and compiler
  mechanics. It should not own `"nmp.feed.home"` as the durable identity of an
  application's home timeline.
- Native and browser runtime feed compilation must converge on the same
  `FeedParams -> source graph -> acquisition/projection effects` authority.
  Browser-specific host plumbing is fine; browser-specific product feed
  semantics are not.
- Observed-projection reconciliation should have one reusable owner. The graph
  can drive that owner as an effect, but should not preserve native-only and
  browser-only state machines for the same shape replacement behavior.

## Proposed Internal Model

Add an internal reactive source graph as a Rust substrate, probably near the
native-runtime/feed composition layer first, then factored downward only once the
boundaries prove stable.

Initial scaffolding now lives in `nmp_core::reactive_source_graph`. It provides
typed node ids, input updates, derived values, effect nodes, deterministic
batched propagation, and per-node revisions. `ActiveFollowSet` is now the first
consumer adapter. It uses the graph for active-account/contact-list dependency
tracking while preserving the existing `follows`, `predicate`, and `on_change`
surface that feed-session code consumes. Feed acquisition/projection effects
still use the existing session callbacks; moving those effects behind graph
nodes is the next runtime migration.

Core concepts:

- `SourceNodeId`: stable internal id, usually derived from owner plus source
  kind, not from app-visible strings.
- `SourceNode<T>`: one writer, one typed value, monotonic revision. Values are
  small source facts: `Option<Pubkey>`, `ContactListTruth`, `PubkeySet`,
  `InterestSet`, `FeedResetToken`, or `ProjectionDirty`.
- `DependencyEdge`: node A depends on node B; when B changes, A recomputes
  synchronously on the actor/composition-owned path.
- `SourceReducer`: deterministic reducer from upstream node values to one
  downstream value. It never awaits, never fetches, and never emits FFI.
- `SourceEffect`: explicit internal side effect produced after a node changes:
  replace dependent interests, sync observed projection, reset feed window,
  mark projection changed.

The first graph should be closed and typed, not a generic user-programmable
reactive runtime. App crates continue to declare data such as `FeedParams` and
custom perspective ids. They do not receive `Signal<PubkeySet>`, subscribe to
Rx streams, or write graph operators.

For the active-follows consumer, the graph nodes are:

- `active_account`: value `Option<Pubkey>`, written by the existing identity
  path.
- `active_contact_follows`: latest canonical contact-list follows for
  `active_account`, read from the kernel event store or from the accepted
  kind:3 event currently being fanned out.
- `active_follows`: value `BTreeSet<Pubkey>`, derived from contact truth plus
  self-inclusion when an active account exists.
- `home_feed_source`: value containing admission/attribution predicates or
  their closed-data equivalent, derived from `active_follows`.
- `home_feed_acquisition`: complete child-interest set:
  active account kind:3 source interest plus the primary/derived acquisition
  interest over active follows.
- `home_feed_observed_shape`: live observed projection shape for replay/future
  delivery to the feed engine.
- `home_feed_projection`: dirty/reset signal for the app/session-owned
  projection key using the OP feed schema.

The graph dispatcher should process a source invalidation once, topologically:

1. Recompute changed downstream nodes.
2. Coalesce all source effects for the graph turn.
3. Apply `ReplaceDependentInterestSet` once per acquisition owner.
4. Sync observed projection sessions once per observed source.
5. Reset affected feed engines if the feed perspective changed.
6. Mark snapshot/projection dirty once.

This is not view-on-view subscription. Feed projections still read from their
own engine/store/projection state. The graph only coordinates source facts and
the internal acquisition/projection effects that keep them consistent.

## First Consumer Migration Path

Migrate only `FeedScope::ActiveUserFollows` first.

This PR completes the adapter half of that migration: `ActiveFollowSet` itself
is graph-backed, batches active-account and contact-list source inputs in one
turn, suppresses derived no-op wakes, and preserves the existing public Rust
surface. The remaining steps move feed-session acquisition, observed projection
sync, and reset/dirtying effects from callbacks onto graph-owned effects.

1. Introduce a graph wrapper around the existing active-follow machinery.
   Implemented inside `nmp-nip02::ActiveFollowSet`: the graph derives the
   self-included active follow set and emits perspective-change effects.

2. Move the active-follow session wiring in
   `op_feed_defaults/session_compile/resolve.rs` behind a graph-owned source:
   it should declare the active account dependency, contact truth dependency,
   active follows value, live `InterestShape`, and extra acquisition set.

3. Move native and browser active-follow feed compilation onto one source graph
   authority. A signed-out public feed, if product wants one, should be a
   separate feed declaration with its own scope and projection key, not a
   fallback for `ActiveUserFollows`.

4. Replace manual reset-hook fanout in `session_engine.rs` for this scope with
   graph effects. The resulting effects should call the same existing primitives:
   `ObservedProjectionReconciler::sync`, `ReplaceDependentInterestSet`,
   `FeedController::reset`, and `mark_changed_since_emit`.

5. Keep `FeedParams` and `open_feed` unchanged. The app still asks for primary
   kinds, `FeedScope::ActiveUserFollows`, render mode, admission, ranking,
   window, and app-owned projection key.

6. Once the runtime effect path is proven, collapse duplicate active-follow
   storage if the graph value can replace the `ActiveFollowSet` predicate read
   cache without breaking `FollowListProjection` or the OP feed predicate. Do
   this in a later PR only after tests prove parity.

The migration should not change public feed declarations, native facade APIs,
UniFFI surfaces, or the typed projection schemas.

## Non-Goals

- No generic public `Signal`, Rx, async-stream, or observable API.
- No app-provided source reducers or native closures crossing FFI.
- No new raw `open_interest` public door.
- No rewrite of `nmp-planner`; it continues to consume materialized
  `LogicalInterest`s.
- No change to NIP-65 routing, relay-pin semantics, or merge lattice rules.
- No feed-owned profile/thread/media hydration. Secondary data remains owned by
  the component or sibling projection that renders it.
- No attempt to migrate every `FeedScope` in the first PR.
- No change to typed FlatBuffers projection payloads.

## Open Questions

- Should the first graph live in `nmp-native-runtime` as composition substrate,
  or should a small lower crate own only the graph scheduler/types?
- Should graph node values be stored as concrete enum variants for the first
  closed set, or as generic typed slots hidden behind erased internal storage?
- Can `ActiveFollowSet::predicate()` read directly from a graph-owned
  `PubkeySet` later? In this PR it remains a compatibility read cache written
  only from graph effects, so existing predicates stay live without making
  feed-engine admission take graph locks.
- What is the exact invalidation boundary for local follow publishes,
  sign-in prepopulation, relay ingest, and future cache-serve replay sources?
- Should graph effects run entirely on the actor thread, or can the
  native-runtime composition layer remain the owner while still preserving
  deterministic ordering?
- How should graph diagnostics expose node revisions and effect counts without
  becoming a public app API?

## PR-Sized Implementation Slices

1. Add graph scaffolding with no behavior change: implemented in
   `nmp_core::reactive_source_graph` with internal node ids, revision tracking,
   dependency registration, deterministic recompute, and unit tests over
   synthetic nodes.

2. Wrap active-account/contact truth:
   implemented in `nmp-nip02::ActiveFollowSet` as graph nodes for active account
   and active contact follows, fed from the existing identity slot, event-store
   reader, and accepted kind:3 observer seam.

3. Migrate active follows:
   implemented in `nmp-nip02::ActiveFollowSet`; `active_follows` is derived
   through the graph while the existing public Rust surface remains an adapter.

4. Unify active-follows feed source compilation:
   move browser and native active-follow home feed compilation onto one
   graph-backed source compiler. Delete any no-active-account public fallback
   unless it is reintroduced as a separate signed-out product feed.

5. Migrate active-follows feed acquisition:
   graph effect replaces the session-owned dependent-interest set and observed
   projection shape for `FeedScope::ActiveUserFollows`.

6. Migrate feed reset/projection dirtying:
   graph effect resets the feed window and marks the projection dirty exactly
   once per source turn.

7. Move off framework-owned `nmp.feed.home` identity:
   require the first graph-backed consumer to pass an app/session-owned
   projection key while reusing the OP feed schema and typed wire payload.

8. Delete duplicate callback wiring:
   remove only the active-follows-specific manual reset/sync path that the graph
   fully owns, leaving other scopes untouched.

9. Add narrow ratchets:
   block new positive app-facing examples that use framework-owned product keys
   such as `"nmp.feed.home"`; require durable cache/read-model additions to
   state their source-of-truth classification, writer, canonical input, and
   invalidation/recompute path.

## Verification And Oracle Strategy

Use behavior-first oracles, not implementation snapshots.

- Active account switch oracle: A -> B clears A-derived authors immediately,
  opens/seeds B's kind:3 source, and never pulls or renders A's stale follow set
  during the switch-before-kind:3 window.
- Contact replacement oracle: newer kind:3 `{A,B,C}` -> `{A,B,D}` withdraws C's
  child interest, adds D's child interest, leaves A/B slots unchanged, and
  emits one compile invalidation.
- Empty contact-list oracle: an explicit empty kind:3 withdraws active-follows
  acquisition; it must not become wildcard authors.
- Missing contact-list oracle: no active kind:3 fails closed until cache-serve
  or relay ingest supplies truth.
- Projection oracle: visible feed rows after replaying graph effects match the
  current manual `ActiveUserFollows` session path for the same event sequence.
- Teardown oracle: closing the feed session withdraws dependent interests,
  closes observed projections, unregisters feed/projection resources, and drops
  identity/source observers.
- D8 oracle: repeated same-value source invalidations do not re-sync observers,
  replace interests, reset feeds, or emit projection dirtiness.
- Existing gates should include the touched crates' focused tests plus
  `cargo test -p nmp-testing --test doctrine_lint_smoke`. If public symbols move
  or dependencies change, add `cargo build --workspace`.

Ratchet tests should cover:

- rejected app/product projection keys under `nmp.*` unless explicitly
  classified as framework-internal, schema-only, compatibility, or negative
  examples;
- accepted app-owned keys such as `myapp.timeline.home`;
- rejected durable caches/read models without source classification;
- accepted derived indexes that name canonical input, single writer,
  invalidation/recompute behavior, and durability.
