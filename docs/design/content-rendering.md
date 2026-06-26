# Content Rendering

NMP owns reusable content parsing and typed content wire data. Apps own visual
rendering.

## Ownership

- `nmp-content` parses Nostr content into `ContentTree` / `ContentTreeWire`.
- `nmp-content-fixtures` owns committed content-rendering fixture generation.
- Protocol projections embed typed content-tree bytes when a host needs rich
  rendering data.
- Starter/component registry files provide app-owned SwiftUI, Compose, and TUI
  reference renderers.
- Apps own styling, layout, navigation, media playback, link previews, wallet UX,
  and other product presentation.

`nmp-core` does not depend on `nmp-content`; composition code installs the
content parser where a runtime needs kernel-parsed content bytes.

## Content Tree

`ContentTree` is the Rust parsing substrate. `ContentTreeWire` is the host-facing
wire shape. The wire form is stable enough for generated bindings, fixtures, and
registry components; the internal parser can evolve behind it.

The tree covers text, mentions, event/address refs, hashtags, URLs, media,
emoji, markdown nodes, and reserved segment kinds such as invoices. It carries
structure, not view styling.

## Embed Resolution

Embed resolution and quote/reference cards flow through the Rust-owned
reference/claim surfaces and typed projections. Renderers consume resolved data
from snapshots and do not open ad hoc platform subscription loops per visible
card.

## App Components

Reference UI components are copied into app trees through the component registry.
That is intentional: a product changes rendering by editing its own SwiftUI,
Compose, TUI, or web component files rather than registering global mutable
renderers in NMP.

## Limits

- No SwiftUI/Compose/iced/web UI package is framework-owned runtime state.
- No link-preview HTTP fetch lives in `nmp-core` or `nmp-content`.
- No renderer registry in Rust decides platform view composition.
- No product-specific card branch belongs in a reusable NMP crate unless the
  underlying protocol data shape is reusable across Nostr apps.
