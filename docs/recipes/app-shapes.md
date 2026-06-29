# Recipe Book: Common NMP App Shapes

This page covers common app shapes without creating app-specific shortcuts in
shared crates. The default path is:

1. Rust owns product and protocol state.
2. The shell opens a typed feed, dispatches a generated typed action, or claims
   a visible ref.
3. The app root installs one component host/provider.
4. Components render and manage visible resolve/release lifecycle only.

## Ownership Legend

| Layer | Owns |
|---|---|
| Reusable NMP crate | Generic Nostr mechanisms: event storage, routing, NIP modules, `nmp-content`, `refs.profile`, authoritative `refs.event`, derived `refs.event.envelopes`, typed action builders. |
| App Rust core | Product nouns, product policy, app-specific ranking/admission, durable app state, and app projection shape. |
| Platform shell | Rendering, native navigation/widgets, OS capability execution, snapshot decoding, and component-host bridge objects. |
| Runtime/component host | FFI/wasm transport, snapshot callback, typed action doorway, `resolve_ref` / `release_ref`, and provider stack for components. |

## Timeline-Only Viewer

Use for a following feed, author feed, thread feed, or relay-set stream where
rows are ordinary Nostr content.

- Reusable NMP: `nmp-feed`, NIP modules, routing, repost/delete acquisition,
  cache, and replaceable supersession.
- App Rust core: declares `FeedParams`: primary content kinds, acquisition
  scope, admission, ranking, window, and projection key.
- Shell: opens/closes the feed by handle and renders the pushed projection.
- Runtime/host: transports the handle and installs component providers.
- Single writers: NMP writes event facts; the feed session/app projection writes
  projection facts; the shell writes no feed or protocol facts.

```json
{
  "primary_kinds": [1],
  "render": "OpCentric",
  "acquisition": "ActiveUserFollows",
  "admission": "All",
  "ranking": "ChronologicalDesc",
  "window": { "initial_limit": 80 },
  "projection": "nmp.feed.home"
}
```

Do not pass wrapper kinds such as `6` or `16` as primary input. The compiler
derives acquisition below the app boundary.

## Kind-Filtered Explorer

Use for "show kind N by this tag/author/relay set". If it is a feed, expose an
app-owned typed read helper with the primary kind and typed acquisition scope.
Low-level raw-interest APIs are internal acquisition machinery for
already-compiled static non-feed reads; do not make them a product shell API.

- Reusable NMP: validates primary kinds, compiles acquisition, routes relays.
- App Rust core: owns explorer policy such as topic, relay-set id, custom
  admission, or ranking.
- Shell: supplies the selected topic/id and keeps the opaque feed handle.
- Runtime/host: closes by handle; it never re-derives a filter from UI state.
- Single writers: app Rust writes explorer projection facts; NMP writes event
  facts and routing facts.

```json
{
  "primary_kinds": [30023],
  "render": "Flat",
  "acquisition": { "Tag": { "term": "nostr" } },
  "admission": "All",
  "ranking": "ChronologicalDesc",
  "window": { "initial_limit": 50 },
  "projection": "myapp.feed.longform.topic.nostr"
}
```

For action-triggered discovery, copy the pattern in
[28 - Action-triggered subscriptions](../builder-guide/28-action-triggered-subscriptions.md):
`ActionModule` dispatches a typed Claim/Release intent, and the read-session
helper owns the internal acquisition, replay, output, status, and teardown.
Do not expose low-level interest claims or observed-delivery internals as
app-facing product APIs.

## Long-Form Reader

Use for NIP-23 articles, topic feeds, or a direct `naddr` reader.

- Reusable NMP: `nmp-content` parses kind `30023` as markdown, long-form
  projection handles parameterized replaceable supersession, and
  `refs.event.envelopes` carries article embed render data.
- App Rust core: owns reader queues, bookmarks, topic curation, and any
  app-specific article projection.
- Shell: renders `ContentTreeWire` through `NostrContentView` and may enable
  article presentation features such as selection.
- Runtime/host: ref claims resolve visible `naddr` / event refs through
  structured `resolve_ref`, not by parsing tags in UI.
- Single writers: NMP writes article protocol facts; app Rust writes reader
  product facts; the shell writes no content-resolution facts.

```swift
NmpComponentHost(
    profileHost: profileHost,
    embedSource: embedStore,
    eventRefResolver: resolver,
    kindRegistry: kindRegistry
) {
    NostrContentView(
        tree: article.contentTree,
        selectionEnabled: true
    )
}
```

## App-Local Renderer Override

Use when the app wants a different card for an already-resolved projection.
Do not fork `NostrContentView` or parse raw events in a renderer.

- Reusable NMP: `EmbedKindProjection`, `NostrKindRegistry`, component host, and
  derived `refs.event.envelopes`.
- App Rust core: if a new kind becomes product-significant, adds the projection
  variant or app projection that produces the facts.
- Shell: registers a renderer for an existing typed variant or unknown numeric
  kind; it only maps model fields to views.
- Runtime/host: keeps the registry below the app-root provider.
- Single writers: Rust writes projection facts; the renderer writes visual
  presentation only.

```swift
struct MagazineArticleRenderer: KindRenderer {
    func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView {
        guard case .article(let article) = projection else {
            return AnyView(EmptyView())
        }
        return AnyView(MagazineArticleCard(article: article))
    }
}

let registry = NostrKindRegistry.makeDefault()
registry.setArticle(MagazineArticleRenderer())
registry.registerUnknown(kind: 30402, renderer: BadgeRenderer())
```

Install that registry once at the component-host boundary:

```swift
NmpComponentHost(
    profileHost: profileHost,
    embedSource: embedStore,
    eventRefResolver: resolver,
    kindRegistry: registry
) {
    TimelineView(model: model)
}
```

## Rich App-Owned Projection Feeding Components

Use this for a 29er-like app: a Rust app core composes a rich product
projection once, while iOS, Android, TUI, and web shells only decode and
render.

- Reusable NMP: generic NIP/protocol crates such as `nmp-nip29`, `nmp-content`,
  `refs.profile`, authoritative `refs.event`, and derived
  `refs.event.envelopes`.
- App Rust core: owns the product projection, for example group tree, selected
  group timeline, unread policy, moderation policy, and row ordering.
- Shell: decodes the product snapshot, routes taps, and renders NMP content
  components for the embedded content fields.
- Runtime/host: transports the typed app projection and the standard component
  ref rows.
- Single writers: app Rust writes product facts and projection facts; NMP writes
  generic protocol/ref facts; shells do not reimplement content resolution.

```rust
// Shape only: exact installer names are owned by the live crates.
pub const GROUP_TIMELINE_KEY: &str = "myapp.group.timeline";

pub fn register(app: &mut impl AppHost, groups: GroupStore) {
    install_substrate_and_protocol_features(app);
    myapp_groups::register_protocol_seams(app, groups.clone());

    app.register_typed_snapshot_projection(GROUP_TIMELINE_KEY, move || {
        groups
            .lock()
            .ok()
            .map(|state| encode_group_timeline_projection(&state))
    });
}
```

The projection may contain `ContentTreeWire` fields or ids that components
claim through `refs.event`. It must not ask each platform to rebuild the group
tree, thread model, or embed resolution.

## Native And Web Component-Host Wiring

Install one host/provider at the app or screen root. Do not mount separate
profile/embed provider stacks per row.

- Reusable NMP: registry components, `refs.profile`, authoritative
  `refs.event`, and derived `refs.event.envelopes`.
- App Rust core: produces app projections and any app-specific resolved rows
  that components consume.
- Shell: constructs bridge objects over decoded snapshot mirrors and passes
  them to the provider.
- Runtime/component host: owns the single provider stack and structured
  `resolve_ref` / `release_ref` controls.
- Single writers: Rust writes ref/projection facts; the shell mirrors pushed
  rows for rendering but does not become a second writer.

- SwiftUI: `nmp add component swiftui/component-host`
- Compose: `nmp add component compose/component-host`
- Web: use `NmpComponentHostProvider` from `@nmp/components-web`.

```tsx
<NmpComponentHostProvider
  profileHost={profileHost}
  resolvedEventEmbeds={resolvedEventEmbeds}
  eventRefResolver={eventRefResolver}
  kindRegistry={kindRegistry}
>
  <Timeline rows={rows()} />
</NmpComponentHostProvider>
```

The host/provider consumes:

- `refs.profile` as the profile source projection;
- `refs.event` as the authoritative event-ref row projection;
- `refs.event.envelopes` as derived render data from `nmp-content`;
- generated typed action builders for writes, never hand-spelled namespaces.

Guardrail: [#2257](https://github.com/pablof7z/nostr-multi-platform/issues/2257)
owns the component-host conformance path: fixture rows for `refs.profile`,
`refs.event`, and `refs.event.envelopes`, plus tests/dependency gates that prove
components consume the host/provider and do not import runtime, ABI, worker, or
kernel handles. When that issue lands, use its fixture-backed conformance path
for component examples; this recipe does not add local fixtures.

## Browser Signer And Private-Flow Caveat

Do not duplicate browser signer capability prose here. Browser private flows
are capability-shaped:

- Reusable NMP: signer/runtime crates own NIP-44/NIP-17 capability semantics.
- App Rust core: owns app product policy around sign-in state and message
  actions.
- Shell: collects user input and surfaces runtime failures.
- Runtime/component host: installs signer providers and routes typed action
  bytes; TypeScript UI does not call signer crypto directly.
- Single writers: Rust signer/runtime code writes signer capability facts;
  shells do not add crypto, routing, or private-message fallback policy.

Treat browser private messaging as available only when the active signer mode
advertises the required encryption capability. The single source of truth is the
[browser signer/private-flow capability model](../wasm-surface.md#browser-signerprivate-flow-capability-model),
with the summary matrix in [docs/nips.md](../nips.md). Those are the #2255 docs
landed by PR #2260; link to them instead of restating the capability table.
