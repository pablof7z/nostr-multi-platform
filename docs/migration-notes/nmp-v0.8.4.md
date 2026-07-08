# nmp-v0.8.4 Migration Note

This note is the consumer-facing migration checklist for the `nmp-v0.8.4`
release tag. It complements the durable target guide in `docs/migration.md`;
the target guide explains the final shape, while this file names the concrete
breaks a pinned consumer must handle when crossing this release.

> **Superseded field:** the `.shape(FeedShape::RootIndexed)` example below is
> historical — `RootIndexed` was demolished by #3082/#3086. `FeedShape` now
> has one variant, `Flat`. A consumer crossing a release after `nmp-v0.8.4`
> should read
> [`docs/perf/composite-feed-architecture.md`](../perf/composite-feed-architecture.md)
> instead of copying the `RootIndexed` shape below.

## Deleted Or Renamed Crates And APIs

- Do not depend on `nmp-defaults` or a generated defaults bundle. App Rust code
  is the composition root and must explicitly install substrate, selected
  protocol crates, and app-owned modules.
- Do not use the deleted C/JNI `nmp-ffi` public binding surface as app API.
  Native consumers should move to the UniFFI objects and generated Swift/Kotlin
  bindings over `nmp-native-runtime`.
- App feeds use the declared-feed path. Replace specialized/default feed
  openers with `FeedKey::app(...)`, `feed::events()`,
  `source::active_user().follows()`, and `app.feeds().open_spec(...)`.
- The feed declaration vocabulary is authority-shaped now:
  `FeedShape` names row shape, `source` names acquisition source, `FeedOrder`
  names ordering, `FeedWindowPolicy::bounded(...)` names runtime window policy,
  and `item_projection` is required.

Before:

```rust
let params = FeedParams {
    acquisition: my_authors,
    ranking: old_order,
    window: FeedWindow { initial_limit: 80 },
    render: FeedRender::OpCentric,
};
```

After:

```rust
let handle = app.feeds().open_spec(
    FeedKey::app("app.example.home")?,
    feed::events()
        .kinds([1])
        .source(source::active_user().follows())
        .shape(FeedShape::RootIndexed)
        .order(FeedOrder::NewestByFeedPosition)
        .window(FeedWindowPolicy::bounded(80))
        .item_projection(FeedItemProjection::default()),
)?;
```

## Projection Keys And Schema IDs

- Profile UI reads keyed `refs.profile` rows. Do not read
  `resolved_profiles`, `claimed_profiles`, or `mention_profiles`.
- Event/embed UI reads authoritative `refs.event` rows and derived
  `refs.event.envelopes` render envelopes. Do not read
  `claimed_event_embeds` or parse whole-map event projections in the shell.
- App feed output keys are app-owned projection keys such as
  `FeedKey::app("app.example.home")`. Do not use framework-owned singleton
  feed strings.
- Treat projection schema ids and versions as generated contracts. If a pinned
  consumer mirrors typed rows, regenerate or update the mirror from the release
  instead of relying on stale field names.

## Dispatch Envelopes And Actions

- Shells should not spell action namespaces or JSON payloads by hand for app
  writes. Use generated action builders that return `DispatchEnvelope` bytes.
- Browser writes go through `dispatch_bytes` / `handle_dispatch_bytes` with a
  finished FlatBuffers envelope (`NMPD`), not a wasm-only JSON write vocabulary.
- Native Swift/Kotlin writes go through the generated UniFFI dispatch-byte
  doorway. The generated builder stamps the action namespace, payload schema
  version, and envelope schema version.
- App-owned actions must use app-owned namespaces and their own schema
  contracts. Do not add app product actions to `nmp-core`.

Before:

```swift
bridge.dispatch(namespace: "nmp.nip25.react", bodyJson: body)
```

After:

```swift
let bytes = GeneratedActionBuilders.react(
    correlationId: correlationId,
    targetEventId: eventId,
    reaction: "+",
    targetAuthorPubkey: nil
)
bridge.dispatchBytes(bytes)
```

## UniFFI And Binding Changes

- Swift/Kotlin consumers should construct and hold the UniFFI runtime object
  instead of storing raw native pointers.
- Feed sessions return opaque handles. Hosts pass the handle back for
  `load_older` and `close`; hosts must not reconstruct key-based or
  compiler-specific feed operations.
- `feed_load_older` exposes typed load status (`changed` plus stop reason) on
  the browser surface, so hosts must not infer exhausted or budget-limited
  source state from absence of new rows.
- Generated bindings are checked in and drift-gated. Regenerate bindings from
  this release when updating a pinned native consumer.

## Consumer Checklist

- Re-pin NMP, then run `nmp upgrade --to 0.8.4` in the app root so app-module
  `nmp-*` dependencies move to the release tag shape.
- Replace defaults-bundle setup with an explicit Rust composition root.
- Replace raw feed/open-interest code with the declared-feed API and app-owned
  `FeedKey`.
- Replace legacy projection mirrors with `refs.profile`, `refs.event`, and
  `refs.event.envelopes`.
- Replace handwritten action namespace dispatch with generated
  `DispatchEnvelope` builders.
- Replace C/JNI public binding calls with UniFFI runtime calls or app-owned
  shell glue that wraps the UniFFI/native runtime API.
- Run the app crate tests, native/web binding generation checks, and
  `cargo test -p nmp-testing --test doctrine_lint_smoke` before treating the
  re-pin as complete.
