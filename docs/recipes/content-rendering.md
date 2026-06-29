# Recipe: Nostr Content Rendering With NMP Components

Nostr content arrives as plaintext with mentions, hashtags, links, media, and
event references. Rendering it correctly is shared NMP work, not per-screen app
work. The app chooses the product projection; NMP content components render the
already-projected data.

## Ownership

- Reusable NMP: `nmp-content`, `ContentTreeWire`, component registry entries,
  `refs.profile`, authoritative `refs.event`, and derived
  `refs.event.envelopes`.
- App Rust core: decides which rows exist, which product projection surfaces
  them, and whether a new event kind needs a typed projection.
- Platform shell: decodes snapshots, installs one component host/provider, and
  maps typed render models to native views.
- Runtime/component host: owns `resolve_ref` / `release_ref` and provider
  wiring. Components below it never import runtime, legacy native compatibility
  shims, worker, or kernel handles.
- Single writers: Rust writes content/projection facts; shell renderers write
  only presentation.

## Prerequisites

Clone the NMP repo and install the CLI:

```sh
git clone https://github.com/pablof7z/nostr-multi-platform
cd nostr-multi-platform
cargo install --path crates/nmp-cli
```

## Recipe 1 - Minimal Inline Text

Best for notifications, previews, and compact rows where you only need inline
text, mentions, hashtags, and links.

### Install

```sh
nmp add component swiftui/content-minimal
```

This copies:

- `Components/NostrContent/NostrContentRenderer.swift`
- `Components/NostrContent/NostrMinimalContentView.swift`

### Usage

`NostrMinimalContentView` consumes `NostrContentRun` values. The installed
component includes a `ContentTreeWire.nostrMinimalRuns()` adapter, so flatten
the Rust-owned content tree instead of carrying a second shell-owned content
model:

```swift
struct NotePreview: View {
    let contentTree: ContentTreeWire

    var body: some View {
        NostrMinimalContentView(runs: contentTree.nostrMinimalRuns())
            .lineLimit(3)
    }
}
```

The shell may set colors through `NostrContentRenderer`; it must not re-tokenize
raw Nostr content.

## Recipe 2 - Full Content View

Best for timelines and detail screens where notes may include block markdown,
media, mentions, event references, and embedded cards.

### Host conformance

Full reference-first components expect an app-root component host/provider.
Install it with the fixture role while wiring your shell tests:

```sh
nmp add component swiftui/component-host --with fixture
```

The fixture provides fake `refs.profile`, `refs.event`, and
`refs.event.envelopes` rows so profile and embed components can be rendered
without a live kernel. The production app still supplies its real host bridge at
the app or screen root; component source must not import runtime, legacy native
compatibility shims, worker, or kernel handles directly.

### Install

```sh
nmp add component swiftui/component-host
nmp add component swiftui/content-view
```

For Android:

```sh
nmp add component compose/component-host
nmp add component compose/content-view
```

The content view depends on the kind-dispatch registry. Event refs render
through the app-root component host, which reads `refs.event.envelopes` derived
from authoritative `refs.event` rows.

### SwiftUI Usage

```swift
let registry = NostrKindRegistry.makeDefault()

NmpComponentHost(
    profileHost: profileHost,
    embedSource: embedStore,
    eventRefResolver: resolver,
    kindRegistry: registry
) {
    NostrContentView(
        tree: row.contentTree,
        mentionLabel: { uri in displayLabel(for: uri.primaryId) }
    )
}
```

### Web Usage

```tsx
<NmpComponentHostProvider
  profileHost={profileHost}
  resolvedEventEmbeds={resolvedEventEmbeds}
  eventRefResolver={eventRefResolver}
>
  <NostrContentView tree={row.contentTree} fallback={row.rawContent} />
</NmpComponentHostProvider>
```

### Host Contract

The component host consumes:

- `refs.profile` for profile rows;
- `refs.event` as the authoritative event-ref row projection;
- `refs.event.envelopes` as derived render data;
- an event-ref resolver that forwards visible refs to structured
  `resolve_ref` / `release_ref`.

Do not restore whole-map names such as `resolved_profiles` or
`claimed_event_embeds`. They are not live data sources.

## Recipe 3 - Long-Form Article Body

Best for NIP-23 reader screens. Rust parses kind `30023` as markdown into
`ContentTreeWire`; SwiftUI can opt into article-mode selection and decoration
support by enabling `selectionEnabled` or supplying decorations.

```swift
NostrContentView(
    tree: article.contentTree,
    decorations: article.highlightDecorations,
    selectionEnabled: true
)
```

Article cards inside another note still render through the kind registry:
`refs.event.envelopes` contains the resolved article projection and the
registered renderer decides how it looks.

## Recipe 4 - App-Local Renderer Override

Best when you need to replace one card renderer without forking shared
components.

Create a renderer for the already-resolved projection and register it on the
app-root `NostrKindRegistry`:

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
```

For a numeric kind that does not yet have a typed projection variant, register
an unknown-kind renderer:

```swift
registry.registerUnknown(kind: 30402, renderer: BadgeRenderer())
```

If that kind becomes product-significant across platforms, add a Rust-owned
projection variant instead of teaching every shell to decode raw tags.

## Recipe 5 - Theming

The renderer environment is per subtree, so different surfaces can use
different visual treatment without changing content facts:

```swift
NostrContentView(tree: message.contentTree)
    .nostrContentRenderer(NostrContentRenderer(
        mentionColor: .secondary,
        hashtagColor: .secondary,
        linkColor: .accentColor
    ))
```

Colors, fonts, and spacing are presentation. Routing, embeds, and profile
resolution remain Rust/component-host facts.

## Updating Components

After NMP releases new component versions:

```sh
nmp update component swiftui/content-view
```

Keep custom renderers in app-owned files. Shared component files can then update
without merging a local fork of the renderer kit.

## See Also

- [Common app shapes](app-shapes.md)
- [`crates/nmp-cli/registry/registry.swiftui.toml`](../../crates/nmp-cli/registry/registry.swiftui.toml)
- [`crates/nmp-cli/registry/registry.compose.toml`](../../crates/nmp-cli/registry/registry.compose.toml)
- [Browser signer/private-flow capability model](../wasm-surface.md#browser-signerprivate-flow-capability-model)
