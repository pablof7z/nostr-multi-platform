---
title: Nostr Content Renderer
slug: nostr-content-renderer
topic: ui-components
summary: The `NostrImageLoader` closure in `NostrContentRenderer` provides a protocol seam for image loading, with a default that uses `URLSession` backed by `NostrImage
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-25
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:63dfcbb3-3ae0-48bb-9228-a494f85df203
---

# Nostr Content Renderer

## NostrImageLoader

The `NostrImageLoader` closure in `NostrContentRenderer` provides a protocol seam for image loading, with a default that uses `URLSession` backed by `NostrImageCache` (32MB memory, 256MB disk `URLCache`); apps inject Kingfisher/Nuke by overriding the closure. <!-- [^45258-17] -->

## NostrIdenticon

`NostrIdenticon` renders a 5×5 symmetric pixel-grid pattern (GitHub-style) using djb2 hash lower 15 bits via SwiftUI `Canvas`, replacing the earlier meaningless hex-char initials. <!-- [^45258-18] -->

## Custom Emoji Rendering

Custom emoji rendering uses a pre-populated `emojiImages: [String: UIImage]` dictionary on `NostrContentRenderer`; when an emoji shortcode is present in the dictionary it renders as `Text(Image(uiImage: img))`, otherwise falls back to `:shortcode:` text. <!-- [^45258-19] -->

## Video and Audio Nodes

Video nodes in `NostrContentView` render as an inline `VideoPlayer` with 16:9 aspect ratio; audio nodes render as a compact tap-to-open button. <!-- [^45258-20] -->

## ContentTreeWire.nostrMinimalRuns()

`ContentTreeWire.nostrMinimalRuns()` is an extension method that converts the wire format to `[NostrContentRun]` for `NostrMinimalContentView`, handling all inline node types and skipping block-level nodes. <!-- [^45258-21] -->

## Newline Sentinel and Inline Rendering

The `nostrContentNewlineSentinel` (`UInt32.max`) is used in `NostrContentGrouping` to inject newlines after bare inline root nodes; to render inline, nodes must be wrapped in a `.paragraph(children:)` node. <!-- [^45258-22] -->

## Swift 6 Strict Concurrency

Swift 6 strict concurrency requires `NostrContentRenderer` and `NostrContentCallbacks` to be `@unchecked Sendable`, file-level constants to use `nonisolated(unsafe) let`, and static methods to use `nonisolated static func`. <!-- [^45258-23] -->

## ContentTreeWire and NostrWireNode

ContentTreeWire uses `NostrWireNode` with 20 variants and a forward-compat fallback to `.placeholder(reason: .depthLimit)` for unknown node types. <!-- [^45258-24] -->

## Raw Toggle and Mention Resolution

All content-parsing pages on both iOS and Android have a raw toggle that switches between rendering the actual `nostr:npub1…` wire URI and the kernel-resolved `@displayName` — without synthesizing or fabricating data. No hardcoded fake data (e.g., "jack", "satoshi", "deadbeef" pubkeys, "npub1example" URIs) exists in any content component page on either platform; all display names are resolved from the kernel via `profileMap`. Android `ContentComponentPage` accepts a `GalleryModel` parameter and resolves all mention display names from `profileMap`, falling back to `defaultMentionLabel` when a profile is not yet resolved. Android content mention resolution works end-to-end: in resolved mode the content shows `@PABLOF7z`, and in raw mode it shows the full `nostr:npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft` URI. iOS `NostrMinimalContentView` accepts an optional `mentionLabel` closure threaded through `walkMinimal`, enabling the minimal renderer to show raw wire URIs when the raw toggle is enabled. iOS raw toggle works: in resolved mode it shows `@PABLOF7z`, and in raw mode it shows `@nostr:npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft` — the actual wire URI with no synthesis. <!-- [^63dfc-2] -->

## ContentComponentPages Implementation

Android `ContentComponentPages.kt` includes a `demoMentionTree()` function with the real npub URI so the content-mention-chip page has a `NostrContentView` section that responds to the raw toggle. iOS `ContentComponentPages.swift` uses the real `DEMO_PUBKEY_HEX` and real npub URI in `SampleContent.richTree`, and `ContentQuoteCardPage` and the no-avatar chip use live kernel-resolved profile names. <!-- [^63dfc-3] -->
