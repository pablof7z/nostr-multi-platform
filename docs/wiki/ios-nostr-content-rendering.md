---
title: iOS Nostr Content Rendering & Registry Components
slug: ios-nostr-content-rendering
summary: The SwiftUI registry components include `NostrContentView`, `NostrMentionChip`, `NostrQuoteCard`, `NostrMediaGrid`, `NostrMinimalContentView`, and `NostrContent
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-29
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:a1c3e15c-4d85-4e01-9824-7b7bf6a50c43
  - session:63dfcbb3-3ae0-48bb-9228-a494f85df203
  - session:d98be997-81df-4738-8846-8323d40ab9ff
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:cd331450-f93f-48d0-960e-3c73e927775e
---

# iOS Nostr Content Rendering & Registry Components

## SwiftUI Registry Components

The SwiftUI registry components include `NostrContentView`, `NostrMentionChip`, `NostrQuoteCard`, `NostrMediaGrid`, `NostrMinimalContentView`, and `NostrContentRenderer` environment injection. [^45258-10]

The `NostrContentRenderer` and `NostrContentCallbacks` must be `@unchecked Sendable` for Swift 6 strict concurrency compatibility due to closures and UIImage properties being non-Sendable. [^45258-11]

All `AsyncImage` uses in `NostrMentionChip`, `NostrMediaGrid`, and `NostrQuoteCard` were replaced with `NostrImageView` which delegates to the renderer's `imageLoader` closure. [^45258-12]

The content-mention-chip demo renders 'Hey @pablof7z, how are you?' using NostrContentView with a mentionLabel closure that resolves the display name from the kernel's kind:0 data. [^53838-4]

The NmpUserPreview iOS app source lives at /tmp/NmpUserPreview/ as a standalone Xcode project (NmpUserPreview.xcodeproj) wrapping a Swift package (NmpUserPreviewPackage/) containing registry SwiftUI components. [^a1c3e-1]

`NostrMinimalContentView` accepts an optional `mentionLabel` closure threaded through `walkMinimal` so the minimal renderer can show raw wire URIs when the closure is absent. [^53838-4]

EmbedFrameContext carries the profiles map and wires it through render_body to every NostrContentView that has a claim sink. [^d98be-5]

SwiftUI views use `.task` to open Rust projections and `.onDisappear` to close them as the primary lifecycle pattern. [^54ae9-7]

The iOS typed home feed decoder falls back to the generic path using the chain `typedHomeFeed ?? snapshot?.homeFeed ?? .empty`. [^cd331-7]

<!-- citations: [^45258-10] [^45258-11] [^45258-12] [^53838-4] [^a1c3e-1] [^63dfc-6] -->
## Image Loading

The `NostrImageLoader` is a closure-based seam in `NostrContentRenderer` (`imageLoader: @Sendable (URL) -> AnyView`) with a default that uses `URLSession` over a `URLCache` with 32MB memory and 256MB disk capacity. [^45258-13]

## Identicons

The `NostrIdenticon` renders a geometric 5×5 symmetric pixel-grid identicon using djb2 hash lower 15 bits, rendered via SwiftUI `Canvas` in one pass, replacing the previous meaningless hex-char initials. [^45258-14]

## Custom Emoji Rendering

Custom emoji rendering uses an `emojiImages: [String: UIImage]` dictionary on `NostrContentRenderer`; when available, it renders `Text(Image(uiImage: img))`, otherwise falls back to `:shortcode:` text. [^45258-15]

## Video and Audio Nodes

Video nodes render as an inline 16:9 `VideoPlayer` from AVKit for video content, while audio remains a compact tap-to-open button. [^45258-16]

## NostrLoginBlock

The `NostrLoginBlock` component detects local signers via `UIApplication.shared.canOpenURL`, supporting Amber (`nostrsigner`), Primal (`primal`), and NIP-46 bridge (`nostrconnect`), with a `case olas` reserved for future use since Olas has no URL scheme. [^45258-17]

## NostrRelayList

The `NostrRelayList` component renders `projections.relay_edit_rows` with connection status dots (green=connected, orange=connecting with pulse animation, red=error, gray=unknown), role badges, and wss://-stripped URLs in monospaced font. [^45258-18]

The `role_tint` field in `RelayEditRow` is a semantic token (`accent`/`info`/`success`/`neutral`) mapped to SwiftUI colors first (accent→.accentColor, info→.blue, success→.green, neutral→.secondary), with hex color fallback for forward-compatibility. [^45258-19]

## Author Picture and ADR-0032

ADR-0032 display separation made `author_picture_url` nullable (null before kind:0 arrives) and removed `author_avatar_source` entirely; the presentation layer owns placeholder/identicon strategy. [^45258-20]

## ContentComponentPages

ContentComponentPages must not contain fake data — all hardcoded display names and fake pubkeys must be replaced with real pubkeys and kernel-resolved profiles. SampleContent.richTree uses real `DEMO_PUBKEY_HEX` and a real npub URI instead of fake example pubkeys. [^53838-4]

A `RawToggle` struct with `@State var rawMode` provides per-page toggle state for switching between rendered and raw views. [^53838-4] [^63dfc-7]
## See Also

