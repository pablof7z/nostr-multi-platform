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

`refs.event` is the authoritative event-reference source. It carries row-delta
updates whose payload is raw event data, and hosts merge those rows into a
`RefEventStore`.

`refs.event.envelopes` is a derived render projection. It is emitted only by
composition roots that can see the merged `refs.event` store and `nmp-content`
(`nmp-ffi`, browser runtime, app Rust proof hosts). The reusable derivation path
is `nmp_content::derive_ref_event_envelopes` or
`nmp_content::derive_ref_event_store_envelopes`, which turns the merged
`refs.event` rows into pre-resolved `primary_id -> EmbeddedEventEnvelope` data
through `resolve_embed_projection`. Typed-frame shells and component registries
consume that sidecar because they cannot run Rust resolver code locally. It is
not a second source of truth and must never be populated from legacy whole-map
event claim projections.

Renderers consume resolved data from snapshots and do not open ad hoc platform
subscription loops per visible card.

## App Components

Reference UI components are copied into app trees through the component registry.
That is intentional: a product changes rendering by editing its own SwiftUI,
Compose, TUI, or web component files rather than registering global mutable
renderers in NMP.

SwiftUI and Compose apps should bind one app-level component host at the app or
screen root. SwiftUI uses
`.nmpComponentHost(profileHost:embedSource:eventRefResolver:kindRegistry:)`.
Compose uses `NmpComponentHostProvider(profileHost:resolvedEventEmbeds:eventRefResolver:kindRegistry:)`.
These APIs only bundle existing host seams: `NostrProfileHost` reads
`refs.profile`, the embed source/local mirrors derived `refs.event.envelopes`,
the event-ref resolver reports visible resolve/release lifecycle to the app
bridge, and `NostrKindRegistry` dispatches already-typed render projections.
They must not own kernel handles, parse raw Nostr events, or maintain a second
profile/event cache.

## Limits

- No SwiftUI/Compose/iced/web UI package is framework-owned runtime state.
- No link-preview HTTP fetch lives in `nmp-core` or `nmp-content`.
- No renderer registry in Rust decides platform view composition.
- No product-specific card branch belongs in a reusable NMP crate unless the
  underlying protocol data shape is reusable across Nostr apps.
