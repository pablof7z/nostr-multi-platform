# Internal Machinery

The developer-facing shape is small, but NMP still owns the machinery needed to
make it correct across platforms.

## Read Pipeline

For reads, NMP internally:

```text
opens a live query
  -> compiles source expressions into interests/dependent interests
  -> plans relays with outbox routing or explicit relay pins
  -> replays cached/store data
  -> admits matching live events into Rust-owned read state
  -> emits bounded typed projections
  -> tears everything down when the handle closes
```

Protocol crates own the meaning of protocol queries. The live query machinery
owns lifecycle, replay ordering, dependency tracking, and teardown.

The current `ObservedProjection` path is the closest internal primitive and
should be reused. The goal is not a parallel read path or a broad new engine. It
is to prove that a small typed descriptor can compile into the safe recipe
feature authors need.

Dynamic source implementations should be consolidated only as far as their
semantics truly match. Start with one private shape reconciler around
observed-projection open/close, keep source-specific reducers local, and promote
a general source-reduction core only after multiple source families prove they
share the same diff, fail-closed, teardown, and dependent-interest rules.

## Write Pipeline

For writes, NMP internally:

```text
receives a typed write intent
  -> constructs or finalizes unsigned event data through the owning feature
  -> signs with the selected signer through the capability/signer port
  -> stores the signed event when appropriate
  -> plans publish relays
  -> dispatches to relays
  -> records publish status and errors as state
  -> updates projections through normal ingest/store paths
```

Protocol crates own protocol-specific draft and route policy. App crates compose
protocol flows when product behavior spans protocols.

Existing one-door pieces should remain the substrate: dispatch envelope, action
modules, signer port, publish policy, publish command, publish engine, local
store, retry/cancel controls, and typed status outputs. `EventDraft` and
`PublishContext` name invariants across those pieces, not automatic new types or
permission to add a second publish stack.

## Actor And Store Rules

The actor remains the single writer. New nondeterministic inputs enter as typed
actions, capability results, or injected seams. Reducers remain replayable from
message history.

The event store and indexes stay inside Rust. Shells receive typed projection
state, not raw store handles. Raw signed events may be exposed through explicit
inspection/export features, but not as the default app data path.

Projection delivery must preserve current hard requirements: typed FlatBuffers
or equivalent schemas, incremental apply, stale-frame drops, tombstones, clear
messages, full-pull cold start paths, bounded replay, output namespaces, and
generated host adapters. Hiding projection tiers from app developers cannot mean
deleting these executor guarantees.

## Complexity Justification Gate

No internal mechanism in this proposal is accepted just because it already
exists. Before implementation, each mechanism must be red/blue-team reviewed:

```text
mechanism
  -> simplest plausible alternative
  -> invariant defended by the complex design
  -> evidence that the simpler alternative fails
  -> consolidation/deletion opportunity
  -> public/private/compatibility classification
```

The default outcome should be deletion or consolidation unless the mechanism
protects one of these invariants:

- replay correctness: cached data is seen before future live data;
- privacy: private events cannot leak to public relays;
- routing correctness: outbox, inbox, host relay, and explicit routes are not
  guessed by shells;
- single-writer state: product facts have one Rust owner;
- cross-platform parity: Swift, Kotlin, TypeScript, TUI, and browser shells do
  not reimplement protocol behavior;
- bounded reactivity: hot paths do not poll, wake unnecessarily, or ship
  unbounded snapshots;
- downstream necessity: `nmp-gallery`, Highlighter, or Podcast Player cannot
  express a real flow with the simpler design.

Examples of complexity that must be justified, not inherited:

- whether `ObservedProjection` needs its current shape or can be a smaller
  replay-before-live helper under sessions;
- whether `ReducedSource`, dependent interests, pointer sources, and active
  observed projection controllers can collapse into one source-reduction core;
- whether projection tiers need to remain as executor internals, and whether the
  app-facing output manifest can hide them completely;
- whether opening a session can be the scoped output declaration, leaving global
  declared projections only for always-on app chrome or compatibility;
- whether generated host adapters are enough, or whether hand-authored row
  caches still have any valid ownership;
- whether `PublishContext` is a real missing type or only a naming layer over
  existing publish target, behavior, command, and policy data;
- whether off-actor feed mutation and feed-render memo/provider machinery can be
  deleted by routing viewport/load-older through actor/session actions;
- whether live counts can remain typed projections instead of a dedicated
  primitive;
- whether compatibility APIs such as raw `open_interest` need to remain public,
  and if so which scopes may call them.

The ADR should record rejected simpler alternatives. If the only defense is
"this is how the current code works," the mechanism is not justified.

## Non-Goals

- Do not expose a generic raw event callback as the main app API.
- Do not make `open_interest` the app read model.
- Do not let native compute dynamic source sets, route relays, or mutate event
  tags for protocol correctness.
- Do not collapse every protocol-specific publishing rule into `nmp-core`.
- Do not turn `LiveQuery` into an object that owns protocol meaning. Protocol
  and app crates own meaning; live query machinery owns lifecycle.
- Do not present this document as shipped API before ADR and migration work land.
- Do not move product domains into NMP crates because one downstream app needs
  them.
- Do not let compatibility paths remain public teaching examples once the typed
  lifecycle exists.

## Migration Milestones

1. Red/blue-team the complexity budget. Map `FeatureSession`/`LiveQuery`,
   `ObservedProjection`, `ReducedSource`, unsigned-event finalization, publish
   routing context, and live-count output to existing symbols, compare them
   against simpler alternatives, and declare which public APIs become
   substrate/debug/test/expert-only.
2. Extract the smallest private shape reconciler around observed-projection
   open/close and prove one descriptor-backed session compiles into it. Preserve
   muted replay before activation and close interest plus observer together.
3. Delete tick-observer dependencies where event hooks already exist. Keep tick
   observers only as compatibility, or with evidence that a required readiness
   transition has no event source and cannot cheaply gain one.
4. Test whether source-specific reducers can share one source-reduction core.
   Promote the core only for families with the same diff, fail-closed, teardown,
   and dependent-interest semantics. Empty demand must fail closed unless the
   feature declares a fallback source.
5. Make session open the scoped output declaration. Keep global declared
   projections only for always-on chrome, compatibility, or measured wins that
   session-scoped demand cannot reproduce.
6. Migrate component refs and gallery embeds first. This proves profile/event ref
   sessions, URI decoding, generated caches, and no shell retry timers.
7. Migrate feed, group, search, pointer-source, and live-count outputs.
   Composite sessions should use the same descriptor model.
8. Define generated write builders over existing action and publish machinery.
   Preserve signer continuations, private routing, explicit relay policy, and
   read-your-writes tests.
9. Migrate downstream app roots. Highlighter and Podcast Player install explicit
   NMP features plus app-owned feature bundles instead of treating defaults as
   the architecture.
10. Delete or demote old teaching surfaces. Builder docs, examples, and starter
   apps should teach typed sessions/actions, not raw interest/projection recipes.

The same pass must correct durable docs in place. Likely targets include
architecture API-surface docs, overview and DX docs, builder-guide pages for
subscription planning, publish and ledger, walkthroughs, action-triggered
subscriptions, and ADRs that currently expose projection tiers or defaults as
app-facing concepts.

## Fitness Checks

The destination is not reached until these are true:

- No public builder guide asks product apps to manually pair raw interest open,
  observer registration, replay, projection sidecar, and teardown.
- No production state reconciliation depends on snapshot tick polling.
- New app reads enter through typed session descriptors or documented expert
  acquisition scopes.
- Every dynamic source has deterministic diffing, explicit fallback policy, and
  no wildcard result from empty demand.
- Projection tiers are absent from app-facing docs and starter apps.
- Generated host adapters are the only owner of rendering row/delta caches.
- Publish builders route through the existing typed action doorway and publish
  engine.
- Publish/action status is visible as typed Rust-owned output.
- `nmp-gallery`, Highlighter, and Podcast Player can express their current core
  flows without native-owned protocol state or app-domain logic inside NMP.
- Clean-room app docs can build a simple app without reading ADRs, issues, wiki
  pages, or old design chats.
- Every retained internal mechanism has a written invariant, rejected simpler
  alternative, and deletion/consolidation decision.

## Follow-Up ADR Decisions

The ADR for #2316 must settle:

- final names for `LiveQuery`, `LiveView`, `ProjectionSession`, or another term;
- whether the public app door is one generic `open_query` or typed per-feature
  open helpers backed by one descriptor model;
- how existing feed, group, search, ref, and pointer-source sessions migrate;
- how projection producer ownership replaces public Tier-1/Tier-2 language;
- how event draft construction, signer selection, and publish routing are
  represented in generated builders;
- what doctrine lint blocks reintroduction of raw observer, tick, or polling
  recipes.
- how `register_defaults()` is positioned after explicit feature composition is
  the taught model;
- which compatibility APIs remain available and what scopes may call them.
