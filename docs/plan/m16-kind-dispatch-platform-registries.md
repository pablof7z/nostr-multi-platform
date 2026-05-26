# M16 Kind Dispatch: Platform Registries

This page expands F-CR-05, F-CR-06, and F-CR-07. The three platform registries
share the same shape: a registry maps `EmbedKindProjection` variants to renderers,
`EmbeddedEvent` hosts one envelope, and `EmbedChromeContainer` provides border,
indent, depth, and collapsed-state chrome.

## Shared Contract

Every platform registry must provide:

- Typed renderer slots for `ShortNote`, `Article`, `Highlight`, and `Profile`.
- An unknown-kind map keyed by numeric kind for custom handlers.
- A default short-note renderer so quoted notes work out of the box.
- A default unknown renderer so unsupported kinds render honestly.
- A host view/widget/composable named `EmbeddedEvent`.
- Chrome that knows nothing about event content.

`EventRef` rendering must fall back to the legacy quote-card path or an unresolved
placeholder when no envelope is available. It must not invent a successful embed.

## F-CR-05: SwiftUI

Registry component:

`crates/nmp-cli/registry/swiftui/content-kind-registry/`

Expected files:

- `NostrKindRegistry.swift`
- `EmbeddedEvent.swift`
- `EmbedChromeContainer.swift`
- `KindRenderer.swift`
- `NostrKindRegistry+Environment.swift`
- `DefaultShortNoteRenderer.swift`
- `DefaultUnknownRenderer.swift`

SwiftUI requirements:

- `NostrKindRegistry` is an `ObservableObject` propagated through the environment.
- Renderer protocols expose typed projection bodies rather than raw events.
- `register(_:forKind:)` registers an `UnknownKindRenderer` for a numeric kind.
- `EmbeddedEvent` receives `EmbeddedEventEnvelope?`, checks collapse state, and
  delegates projection rendering through the registry.
- `NostrContentView` replaces `eventRefView` with `EmbeddedEvent`.
- `quoteCardProvider` remains as a deprecated one-release compatibility path.

## F-CR-06: TUI

Registry component:

`crates/nmp-cli/registry/tui/content-kind-registry/`

Expected files:

- `nostr_kind_registry.rs`
- `kind_renderer.rs`
- `embedded_event.rs`
- `embed_chrome_container.rs`
- `mod.rs`

TUI requirements:

- `KindRenderer` is object-safe and provides `render` plus `preferred_height`.
- `NostrKindRegistry::make_default()` installs short-note and unknown fallbacks.
- `register_unknown(kind, renderer)` overrides only that kind's unknown fallback.
- `EmbeddedEvent` implements ratatui rendering over `EmbeddedEventEnvelope`.
- `EmbedChromeContainer` draws the left border, indentation, and collapsed placeholder.
- `NostrContentView` accepts a registry and an envelope map. It uses `EmbeddedEvent`
  only when both exist; otherwise it falls back to `NostrQuoteCard`.

This is the first platform slice implemented by PR #588.

## F-CR-07: Compose

Registry component:

`crates/nmp-cli/registry/compose/content-kind-registry/`

Expected files:

- `NostrKindRegistry.kt`
- `EmbeddedEvent.kt`
- `EmbedChromeContainer.kt`
- `KindRenderer.kt`

Compose requirements:

- `NostrKindRegistry` is provided through a `CompositionLocal`.
- `KindRenderer` is a composable interface that receives the typed projection,
  render context, registry, and tap callback.
- `EmbeddedEvent` receives `EmbeddedEventEnvelope?`, computes render context, and
  wraps registry output in `EmbedChromeContainer`.
- `WireNode.EventRef` calls `EmbeddedEvent` after F-CR-02 moves Android to
  `ContentTreeWire`.
- The old `EmbedCard.kt` path is deleted in the same slice.

Verification is platform-specific:

```sh
# TUI
cargo test -p chirp-tui --lib
cargo test -p nmp-cli --test e2e_tui

# iOS
xcodebuild test -scheme Chirp -testPlan ContentRendering

# Android
./gradlew :gallery:test :gallery:connectedAndroidTest
```
