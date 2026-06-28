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
should be reused. The goal is not a parallel read path; it is to lift the safe
recipe into a descriptor-driven lifecycle that feature authors use consistently.

Dynamic source implementations should collapse into one source-reduction core:
follow/list/group/member sources, pointer sources, visible refs, browser feed
sources, app-specific sources, and count sources all use the same diff and
dependent-interest semantics.

## Write Pipeline

For writes, NMP internally:

```text
receives a typed write intent
  -> constructs or finalizes an EventDraft through the owning feature
  -> signs with the selected signer through the capability/signer port
  -> stores the signed event when appropriate
  -> plans publish relays
  -> dispatches to relays
  -> records publish status and errors as state
  -> updates projections through normal ingest/store paths
```

Protocol crates own protocol-specific draft and route policy. App crates compose
protocol flows when product behavior spans protocols.

Existing one-door pieces should remain the substrate: dispatch envelope,
action modules, signer port, publish policy, publish command, publish engine,
local store, retry/cancel controls, and typed status outputs. `EventDraft` and
`PublishContext` are names for the missing shape across those pieces, not
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

1. Map terms to current code. Tie `FeatureSession`/`LiveQuery`,
   `ObservedProjection`, `ReducedSource`, `EventDraft`, `PublishContext`, and
   `ReactiveCount` to existing symbols and declare which public APIs become
   substrate/debug/test/expert-only.
2. Wrap one existing observed projection as a descriptor-backed session. Preserve
   muted replay before activation and close interest plus observer together.
3. Extract reusable source reduction from pointer/dependent-interest/feed
   patterns. Empty demand must fail closed unless the feature declares a fallback
   source.
4. Replace tick-based observed projection reconciliation with event-driven
   reconciliation from identity, source, mailbox, store, and refcount events.
5. Add typed output manifests over `SnapshotRegistry` and declared projections.
   Generated adapters own row/delta caches and clear semantics.
6. Migrate component refs and gallery embeds first. This proves profile/event ref
   sessions, URI decoding, generated caches, and no shell retry timers.
7. Migrate feed, group, search, pointer-source, and reactive-count features.
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
