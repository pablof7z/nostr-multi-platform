---
title: Cross-Platform Kind Rendering Dispatch
slug: cross-platform-kind-rendering-dispatch
summary: Event embedding across platforms uses a single rendering engine that dispatches by kind, so embedded events render with the same kind-specific renderer as top-l
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-29
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:1572547f-2b2d-49fb-a383-e95ca25d0bc3
  - session:fa300009-e498-4c80-a2d3-64d1531a09d4
  - session:9de494e6-e783-4785-ae67-1f7014dadd5d
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:6e8af009-f065-464a-98f1-3ec1ee4ed933
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# Cross-Platform Kind Rendering Dispatch

## Cross-Platform Kind-Dispatch Rendering

Event embedding across platforms uses a single rendering engine that dispatches by kind, so embedded events render with the same kind-specific renderer as top-level events. Each Nostr kind has its own renderer, and the kind-to-renderer dispatch works both for top-level events and for events embedded inside other events. The kind dispatch registry must support easily expanding to hundreds or thousands of kind renderers (e.g., classified events, zap receipts, mute lists, NIP-29 group metadata). [^15725-1]


## Embed Shell vs. Content Rendering

The embed shell for an event reference is visual chrome (border/indent/depth cues) and does not own content rendering. Embedded event widgets render with a full rectangular box border to clearly distinguish them as nested events. EmbedChromeContainer ships inside the platform-specific F-CR-05/06/07 items and knows nothing about content, providing a depth-graded chrome view for embeds. On iOS, embedded event widgets use a lineWidth of 1.5 instead of the previous invisible hairline of 0.5. On Android, embedded event widgets use a border width of 1.5.dp instead of the previous invisible hairline of 0.5.dp for all three card variants (collapsed, frame, missing). In the TUI, the embedded event widget uses Borders::ALL instead of Borders::LEFT, its preferred_height accounts for 2 rows for top/bottom borders, and it calculates inner width by subtracting 2 for borders and 2 for body indentation.

Event embeds must render inline within their surrounding note text (e.g., 'hey, check out my article [card] I hope you enjoy it!'); raw nostr: URI text, card swallowing prose, or missing surrounding text are all failures.

ContentView renders embedded events inline at the exact EventRef position within the content tree. NostrContentView uses an embed dispatch path to render quoted events instead of collapsed placeholders. ContentView handles inline embed dispatch for all embed kinds: Article renders as ArticleCard, ShortNote renders as author plus content preview, Highlight renders as highlighted text plus source link, and Profile renders as display name chip. EmbeddedEvent manages the claim/release lifecycle view for embedded content. ChirpAvatar requires a pubkey: first parameter for the claim/release lifecycle. ChirpAvatar uses raw AsyncImage instead of NostrImageCache for image loading.

<!-- citations: [^9de49-5] [^15725-2] [^fa300-1] [^6a951-4] [^38935-3] -->
## Kind-Dispatch Registry and Wire Format

The kind dispatch registry maps EmbedProjectionVariant to widget, not raw kind numbers to widget, so that Rust decides which variant (which kind maps to what data shape) and native code decides which widget renders that variant. Rust owns the EmbedKindProjection enum, ContentTreeWire is the single wire format, and nmp-content::RenderContext is the single recursion guard. The TUI and SwiftUI Content Kind Registries provide runtime dispatch of EmbedKindProjection variants to platform-appropriate renderers, pre-populated with defaults and allowing host apps to swap slots. [^15725-3]

<!-- citations: [^15725-3] [^54ae9-3] -->
## Platform-Specific Registry Implementations

The iOS kind registry is an ObservableObject with a KindRenderer protocol propagated via @Environment(\.nostrKindRegistry). NostrKindRegistry provides a kind-to-renderer dispatch table for embedded events. NostrProfileHost is defined in its own NostrProfileHost.swift file, not inline in ProfileWire.swift. The TUI kind registry uses a HashMap of KindRenderer trait objects (Send + Sync) propagated via a .kind_registry setter. The iOS quoteCardProvider closure API is kept @available(*, deprecated) for one release to avoid breaking Chirp before migration lands. Chirp must add NostrKindRegistry and related embed components (EmbedKindProjection, EmbedChromeContainer, ArticleEmbed for kind:30023, HighlightEmbed for kind:9802) to support extensible kind-to-renderer dispatch. ChirpApp wires environment injection for the embed system. HighlightEmbed provides a rich renderer for kind:9802 highlights with pull-quote styling.

<!-- citations: [^9de49-4] [^15725-4] [^9a2c7-10] [^38935-2] -->
## Extensibility and Data Boundaries

The Unknown variant in EmbedKindProjection is the extensibility primitive allowing native handlers to render arbitrary kinds by checking projection.kind and pulling data from tags and content_tree, without requiring a Rust change. EmbedKindProjection variants carry only raw protocol data (hex pubkeys, u64 timestamps, verbatim kind:0 strings); native widgets compute derived display values like initials, color, abbreviated npub, and relative time themselves.

The Embed Claim Registry provides refcounted claim/release for embedded Nostr events, deduplicating in-memory state when multiple UI components render the same nevent1/naddr1. Phase 1 performs dedupe and refcount only without opening/closing upstream subscriptions; Phase 2 will wire claims to drive subscription open/close with grace-period teardown. The EventClaimSink trait bridges renderer claim/release to the FFI surface (nmp_app_claim_event / nmp_app_release_event), with each platform host supplying its own implementation. Embedded article/note/highlight showcases wire up the real EventClaimSink pipeline where event refs in content trees are claimed every tick, EmbedHostState decodes the kernel's claimed_events projection, and resolved items render as proper cards instead of placeholders. The detail pane renders content-* components via synthetic GalleryData tree snippets and embed-* as placeholders.

<!-- citations: [^15725-5] [^54ae9-4] [^6e8af-1] -->
## See Also

