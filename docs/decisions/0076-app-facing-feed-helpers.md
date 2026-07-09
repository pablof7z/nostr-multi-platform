# ADR-0076: App-facing feed APIs as typed read-session helpers

## Decision

NMP exposes feed-shaped product reads as typed read-session helpers over
[ADR-0070](0070-typed-read-sessions.md).

`FeedParams` is the canonical serializable descriptor. Ergonomic builders and
generated Swift/Kotlin/TypeScript helpers compile to `FeedParams`; they are not
a parallel lifecycle model. Helper APIs call the same session doorway and
standard compiler path.

The normal feed lifecycle is handle-owned:

```rust
let handle = app.feeds().open_spec(feed_key, feed_spec)?;
app.feeds().load_older(&handle);
app.feeds().close(&handle);
```

Normal app, native, browser, TUI, and generated helper code does not pass a
`FeedCompiler`, observer registrar, source-effect hook, raw filter JSON,
projection registrar, pull controller, or teardown recipe.

Dynamic sources are session-owned. Apps declare source expressions; sessions
reconcile active-user follows, hosted groups, list members, relay sets, mute
sets, WoT sets, and other dynamic families. Empty dynamic source sets fail
closed unless the descriptor explicitly declares a fallback.

Feed rows carry a delivery-tagged typed ref vector (`FeedRow.refs:
Vec<TypedRef>`, `crates/nmp-feed/src/typed_ref.rs`): a `RenderOnly` ref is
declared into the `refs.event`/embed render channel only (the existing D7
lane); a `Delivered` ref folds its target into the SAME feed session's own
acquisition so it re-enters as a real delivered event. Profiles, event embeds,
reply counts, thread hydration, media, badges, reaction summaries, and other
component-specific claims stay owned by the component, projection, or sibling
typed session that renders or calculates with those refs — the feed declares
refs, it never resolves them.

Trellis remains private reconciliation machinery. No public feed API exposes
Trellis types or asks callers to assemble Trellis graphs.

## Composite feeds (#3082/#3086)

A composite feed is an additive set of **lanes** over one `FlatFeed`-style
engine (`CompositeFeedParams`/`FeedLane`, `crates/nmp-feed/src/composite.rs`).
Today's single-scope `FeedParams` feed is the degenerate one-lane case; nothing
about it changes. Two additive engine changes make this possible:

1. **Expand arity** — the item builder is `Fn(&KernelEvent) -> Vec<Row>` (was
   `Option<Row>`): one source event can fan into zero, one, or many rows.
2. **Delivery-tagged refs** — see above; this is the mechanism that lets a
   lane's row (e.g. a repost wrapper) hydrate to its real target inside the
   SAME feed rather than through a second store-peek path.

A lane names an acquisition scope, a kind/tag match filter, and an opaque
registered `LaneMappingId` — never a native closure crossing FFI. Protocol
crates register extraction mappings under framework-owned ids (`nip18.target`,
`nip22.root`); `nmp-feed` registers only its own kind-blind identity mapping
(`feed.authored`, the zero-config default lane). Mapping closures are
constructed in Rust at the composition root (ADR-0069) and never cross FFI —
the engine never learns a kind. `NmpApp::open_composite_feed`
(`crates/nmp-native-runtime/src/composite_feed.rs`) is the composition-root
entry point, mirroring `open_feed`'s lifecycle. See
[`docs/perf/composite-feed-architecture.md`](../perf/composite-feed-architecture.md)
for the full design record.

The former `FeedShape::RootIndexed` reply-rollup shape is deleted, not
generalized: `FeedShape` has one variant, `Flat`. Reply-rollup ("so-and-so + N
replied") is not a feed shape; a reply digest, if wanted, is a concept-owned
read grouping delivered rows.

## Context

Feeds are common enough to deserve ergonomic helpers, but they do not get their
own architecture. They are product-shaped read sessions with feed-specific
descriptor fields and generated binding conveniences.

The lower-level feed implementation contains useful mechanics: bounded
windows, the flat row shape, pull paging, feed-session compilation, source
expressions, feed handles, delivery-tagged row refs, lane mappings, and
platform feed facades. Those mechanics stay below the helper surface.

## Consequences

Apps get a humane feed API without learning compiler or observer wiring.
Generated helpers can cover common source families while remaining descriptors
over the same runtime path.

Breaking changes to `FeedParams`, `CompositeFeedParams`, source expression
shape, generated helper families, or handle-owned lifecycle require updating
this ADR and the API docs together.

## Boundaries

Permitted:

- `app.feeds().open(...)`, `app.feeds().open_spec(...)`, and generated helpers;
- direct `FeedParams` descriptors for serializable/binding surfaces;
- explicit app-owned feed keys;
- typed source expressions and bounded window policies;
- row pointers for secondary owners to hydrate.

Forbidden:

- raw `open_interest` as feed API;
- app code passing compilers, observer registrars, source-effect hooks,
  projection registrars, pull controllers, or teardown recipes;
- special singleton feed lifecycle outside helpers;
- wrapper or maintenance kinds as app primary input;
- feed declarations that own profiles, embeds, counts, target hydration, media,
  or component-specific claims;
- Trellis leakage in app/native/browser feed APIs.

## Enforcement

Clean-room docs and CLI template gates reject raw feed/read machinery in app
guidance. Public surface tests keep generated helper families aligned with
`FeedParams` and prevent old singleton or compiler-taking paths from becoming
the normal API.

Feed session tests cover descriptor parsing, fail-closed empty sources, handle
close, pagination by handle, bounded output, and wrapper/maintenance-kind
rejection.

## Related

- [ADR-0070](0070-typed-read-sessions.md) - parent read-session rule.
- [ADR-0075](0075-trellis-private-reconciliation-substrate.md) - private
  reconciliation boundary.
- [docs/product-spec/api-surface.md](../product-spec/api-surface.md) - public
  feed examples.
- [docs/ffi-surface.md](../ffi-surface.md) - binding helper surface.
- #1626 - app-facing feed helpers.
- #2723 - generated helper coverage and external consumer pinning.
- #2746 - ADR current-only cleanup.
