---
title: Android Nostr Content Rendering
slug: android-nostr-rendering
summary: The Android Nostr renderer must have feature parity with the iOS Nostr renderer.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:29d2c220-a86b-4b0d-82fb-d40d8fd4505e
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:63dfcbb3-3ae0-48bb-9228-a494f85df203
  - session:cd331450-f93f-48d0-960e-3c73e927775e
---

# Android Nostr Content Rendering

## Feature Parity

The Android Nostr renderer must have feature parity with the iOS Nostr renderer. [^29d2c-2]


## Inline and Block Rendering

The SegmentDtoView composable renders inline text, hashtags, URLs, mentions, emoji, and invoices, plus block media, event refs, and markdown. Profile mentions render inline as @npub1… indigo chips. Event references render as bordered placeholder cards. [^29d2c-3]

## Markdown Rendering

MarkdownNodeView and InlineFlow render CommonMark blocks including headings, paragraphs, lists, blockquotes, code, and rules. [^29d2c-4]

## Embed Cards and Depth Guards

EmbedCard implements PD-015 depth and cycle guards as an exact port of the iOS RenderContext logic. [^29d2c-5]

## Identicon Algorithm

The Identicon composable uses a byte-for-byte identical algorithm to the iOS implementation. [^29d2c-6]

## NostrRichText Tokenizer

NostrRichText.kt uses an exact tokenizer port from iOS with case-insensitive nostr: scan, greedy [a-z0-9]+ matching, and HRP classification. [^29d2c-7]

## Timeline Integration

NostrRichText is wired into TimelineScreen replacing the raw Text() call. [^29d2c-8]


The Android implementation migrates the stale generic render model (ChirpTimelineSnapshot with blocks/cards) to the OP-centric ChirpOpFeedSnapshot. [^cd331-2]
## Entity Resolution

Entity resolution in the live Android renderer is deferred until the FFI decode_nostr_entity function is added. When a profile has not yet been resolved, NostrMentionChip falls back to a short pubkey display via defaultMentionLabel. Android ContentComponentPage accepts GalleryModel and resolves all display names from the kernel via profileMap, removing all hardcoded fake names.

<!-- citations: [^29d2c-9] [^63dfc-1] -->
## Compose Registry Components

Compose registry components include `NostrContentView` with `AnnotatedString` + `ClickableText`, `NostrMentionChip` with avatar via Coil and identicon fallback, `NostrMediaGrid` with count-driven 1/2/3/4+ layouts, and `NostrQuoteCard` with Collapsed/Compact/Rich/Missing variants. [^45258-2]

## Raw Mode Toggle

Every content-parsing page on both iOS and Android has a raw toggle that toggles between raw wire URI and kernel-resolved display name rendering. The raw toggle applies to all content-parsing views on both iOS and Android, including content-view, content-mention-chip, and content-minimal. In raw mode, the mention is rendered as the actual nostr:npub1... URI from the wire without synthesis or fabrication. In resolved mode, the mention is rendered as @displayName using the kernel-resolved kind:0 profile. Android NostrContentView accepts a rawMode: Boolean parameter that causes WireNode.Mention to render node.uri.uri instead of the resolved mentionLabel. [^63dfc-2]

## Demo Data

Android ContentComponentPages.kt demoMentionTree() uses the real npub URI nostr:npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft. [^63dfc-3]
## See Also

