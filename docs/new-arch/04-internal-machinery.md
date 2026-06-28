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

Projection delivery must preserve hard invariants: typed schema identity,
stale-frame drops, clear/tombstone semantics, full-pull cold start paths,
bounded replay, deterministic row ownership, and one merge contract per output.
Hiding projection tiers from app developers cannot mean deleting these executor
guarantees.

The mechanisms are not automatically sacred. FlatBuffers, sidecar registration,
projection manifests, output namespaces, incremental apply, and generated host
adapters each need an invariant and a rejected simpler alternative. Keep them
where they are the cheapest way to preserve cross-platform decode, stale-frame
protection, render-cache correctness, or wire/CPU bounds. Collapse or delete
them where session-scoped output demand gives the same guarantee with less
surface area.

## Existing Primitive Mapping

The new model should reuse or retire current primitives deliberately:

- `LogicalInterest` and the interest registry remain acquisition internals.
  Product apps should not compose them directly.
- cache-serve and store replay remain hydration internals behind observed
  sessions.
- `ObservedProjectionRegistrar` is the replay-before-live primitive to prove
  descriptor sessions against first.
- dependent interests are the current dynamic-source substrate; they either stay
  internal or collapse into a smaller reconciler/source core.
- `SnapshotRegistry`, `DeclaredProjections`, typed sidecars, incremental apply,
  and `UpdateFrame` remain executor machinery until session-scoped demand proves
  which pieces can be deleted.
- `ActionModule`, `DispatchEnvelope`, `PublishTarget`, publish policy, signer
  ports, and the publish engine remain the one write doorway.
- explicit relay seams remain audited route policy, not native relay choice.

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
  caches can conform to one shared merge contract without becoming product
  state;
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

## Implementation Plan

Each phase must leave the repo shippable and reduce at least one public concept,
duplicate lifecycle recipe, or hidden desync state.

**P0: Baseline and freeze old patterns.**
Inventory public read/write doors and duplicate lifecycle recipes in
`nmp-core`, `nmp-defaults`, `nmp-native-runtime`, `nmp-browser-runtime`,
`nmp-ffi`, `nmp-codegen`, `nmp-gallery`, Highlighter, and Podcast Player. Add or
extend grep gates so new product code cannot add raw `open_interest` app reads,
new tick reconcilers, native relay timers, or native publish/tag construction.
Classify every existing raw read/write door as substrate, protocol-internal,
diagnostic/test, migration shim, or product API to remove/formalize.
Gates: `cargo test -p nmp-testing --test doctrine_lint_smoke` and
`cargo test -p nmp-testing --test feed_public_surface_retired`.

**P1: Prove a descriptor over existing safe machinery.**
Add the smallest private descriptor facade that compiles into
`ObservedProjection::from_shape`, `OpenObservedInterest`, replay limits,
consumer ids, relay pins, and close. Do not add a new lifecycle engine. Use one
real session as proof. Acceptance: replay-before-live and close-both invariants
still pass in observer replay, descriptor idempotence, reducer parity, and
`nmp-defaults` feed open/close tests; no new public API is taught.

**P2: Extract shape reconciliation and delete tick use where events exist.**
Consolidate the duplicated open/close-on-shape-change controllers behind one
private reconciler. Migrate active-account, browser feed, native feed, and
pointer-source controllers only where their semantics match. Delete
`register_snapshot_tick_observer` usage for identity/source/mailbox/refcount
changes that already have event hooks. For each remaining tick observer, either
add the missing explicit event source or document a bounded actor-scheduled
invariant with a staged deletion gate. "Compatibility" alone is not a reason to
keep it.

**P3: Make scoped session demand own scoped output demand.**
Prove that opening a session can declare its typed output. Keep
`DeclaredProjections` only for always-on app chrome, compatibility, or measured
wire/CPU wins. Acceptance: `public_typed_projection_decode` still proves
external decode; generated adapters still handle full, delta, clear, stale-frame,
and baseline semantics.

**P4: Migrate component refs and gallery embeds first.**
Move `ProfileRef`, `EventEmbed`, URI decoding, relay hints, embed envelopes, and
row-delta caches behind typed sessions and generated adapters. Delete shell
retry timers and duplicated URI decoding where the Rust path owns it. This is
the first cross-shell proof because gallery exercises Swift, Kotlin, web, TUI,
and desktop rendering without app-domain policy. Fix gallery's timer-based copy
affordance before treating the registry as a copyable downstream template.

**P5: Migrate dynamic and composite reads only after P1-P4 hold.**
Feed, group, search, pointer-source, thread refs, and live-count outputs move to
the same descriptor model. Source-specific reducers stay local unless a shared
core deletes duplication. Acceptance: feed reduced-source tests, real-relay
reduced-source tests, group/search tests, and empty-source fail-closed tests
cover account switch, source change, relay pin, cache replay, and teardown.

**P6: Collapse write variants by invariant, not by new names.**
Generated builders keep using `DispatchEnvelope` and `ActionModule`. First try
to unify `UnsignedEvent`, `UnsignedEventToRelays`, pre-signed publish, signer
selection, `PublishTarget`, correlation id, and policy validation without adding
new public types. Add a named draft/context type only if it deletes branching or
duplicate route/privacy/protocol state. Gates: publish policy, D10 private
routing, signer continuation, generated builder round-trip, and action-result
tests.

**P7: Prove downstream apps before declaring the architecture final.**
Highlighter must express home feed, room chat, search, comments, share-to-room,
capture, feedback, signer flows, artifact share, article lookup, and room
discussion through app-owned Rust bundles and NMP runtime dispatch. Direct web
NDK relay/filter/tag/sign/publish paths, Swift rich-text protocol projection,
and Swift-owned sync policy must be removed or formally rejected by the ADR.

Podcast Player must express playback, queue, feed subscription, NIP-F4, Blossom
publish, explicit write relays, widgets, settings actions, signer runtime, and
feedback without moving podcast nouns into NMP. Bespoke durable FFI, silent
compatibility paths, stale Swift-store docs, and `nmp-signer-broker` pinning
must converge on generic typed dispatch/projection/capability seams and the
current NIP-46 runtime direction.

`nmp-gallery` must express component refs and embeds without shell protocol
state or timer-based state clearing. It becomes the conformance fixture for
refs/profile, refs/event envelopes, copied/native components, typed dispatch,
and renderer caches only after those constraints hold.

Any downstream flow that requires native-owned policy or a bespoke framework
door is a design failure, not downstream migration debt.

**P8: Correct durable docs and delete compatibility teaching paths.**
Update architecture API-surface docs, overview/DX docs, builder-guide pages for
subscription planning, publish and ledger, walkthroughs, action-triggered
subscriptions, ADRs, wiki pages, and any episode/transcript-derived teaching
material that currently presents projection tiers or defaults as app-facing
concepts. Compatibility APIs may remain only with scope labels, doctrine gates,
and deletion criteria.

## Fitness Checks

The destination is not reached until these are true:

- No public builder guide asks product apps to manually pair raw interest open,
  observer registration, replay, projection sidecar, and teardown.
- No production state reconciliation depends on snapshot tick polling.
- New app reads enter through typed session descriptors or named
  substrate/protocol-internal/diagnostic/test/migration acquisition scopes.
- Every dynamic source has deterministic diffing, explicit fallback policy, and
  no wildcard result from empty demand.
- Projection tiers are absent from app-facing docs and starter apps.
- Rendering row/delta caches implement one shared/generated merge contract.
  Hand-authored caches may exist only as thin, proven adapters; they cannot own
  product facts or independent merge semantics.
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
- whether current `ReducedSource`/`open_feed` machinery is amended, renamed, or
  replaced by a smaller descriptor/reconciler model;
- whether `nmp init` generates a production scaffold or a tutorial preset, and
  how docs label that choice;
- which downstream app migrations are release gates versus follow-up issues.
