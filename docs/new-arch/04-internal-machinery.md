# Internal Machinery

The public model should be small. The internal model still has to preserve the
Nostr and multiplatform invariants that make shipped apps correct.

## Read Pipeline

For reads, NMP internally:

```text
opens a typed session descriptor
  -> compiles source expressions into acquisition demand
  -> plans relays or applies protocol pins
  -> installs admission and wake rules
  -> replays cached/store data before live frames
  -> admits matching live events
  -> reduces Rust-owned read state
  -> emits bounded typed outputs
  -> tears down demand when the handle closes
```

The first implementation should reuse or narrow existing safe machinery before
adding new public concepts: `open_feed`, reference resolution,
`ObservedProjection` registration, dependent interests, output frames, and
current feature sessions are candidates. Old names survive only when they carry a
necessary invariant more cheaply than a new abstraction.

## Write Pipeline

For writes, NMP internally:

```text
receives a typed action or write intent
  -> constructs unsigned event data
  -> finalizes protocol envelope and route provenance
  -> signs through the signer/capability port
  -> stores the signed event when appropriate
  -> plans or applies publish routes
  -> dispatches to relays
  -> records status and errors as state
  -> updates projections through normal ingest/store paths
```

There should remain one publish stack. `EventDraft`, `PublishIntent`, and
`RouteProvenance` name invariants to preserve, not permission to add a second
parallel writer.

## Actor, Store, And Output Rules

The actor remains the single writer. Nondeterministic inputs enter as typed
actions, capability results, relay events, store events, or injected seams.
Reducers remain replayable.

The event store stays inside Rust. Shells receive bounded typed outputs, not raw
store handles or unbounded event history. Raw signed events can be exposed by
inspection/export tools, but not as the default app data path.

Projection delivery must preserve:

- typed schema identity;
- stale-frame drops;
- clear/tombstone semantics;
- bounded replay;
- deterministic row ownership;
- one merge contract per output.

## Runtime Boundary

Native, browser, desktop, and TUI runtimes are capability shells. They render UI,
execute platform APIs, and report raw results. Rust owns product state, route
policy, retry, signer continuation, protocol interpretation, and durable status.

Browser runtime needs explicit worker/storage lifecycle:

```text
load shell
  -> start worker
  -> prepare durable storage if required
  -> install update callback
  -> start Rust app
  -> broker browser-only capabilities such as NIP-07 through main thread
```

Silent fallback to in-memory storage is not a durable product success. Missing
worker, storage, wasm, or signer capability should emit typed runtime failure or
degraded state.

## Retire Or Narrow

The migration should shrink public surface:

- hidden production `register_defaults()` presets;
- raw app-facing `open_interest` read paths;
- public filterless event observers;
- tick-polled projection repair;
- ambiguous `Explicit { relays }` publish routes;
- shell-owned protocol parsing or publish status.

Keep complexity only where it protects a concrete invariant: privacy,
replay-before-live, bounded updates, cross-platform decode, signer safety,
storage durability, or protocol-owned routing.
