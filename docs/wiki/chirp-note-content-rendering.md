---
title: Chirp Note Content Rendering
slug: chirp-note-content-rendering
summary: Chirp's NoteRowView, ThreadNoteRow, and ProfileNoteRow use NoteContentView (a Swift-side tokenizer and renderer) instead of rendering raw item.content Text
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:ec51ad49-af31-4415-aab4-e9123eb63eab
  - session:5d893073-9635-450b-b8e9-50648bc1a4e7
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:b48d81e1-411c-45db-a440-340bcaee2631
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
---

# Chirp Note Content Rendering

## Architecture

Chirp's NoteRowView, ThreadNoteRow, and ProfileNoteRow use NoteContentView (a Swift-side tokenizer and renderer) instead of rendering raw item.content Text. NoteContentView tokenizes content via regex on nostr:, https://, and #tag patterns. [^ec51a-1]



TimelineRow.content stores the full, untruncated note content rather than a preview. [^b48d8-1]

The timeline list view truncates note content to the terminal width at render time. The detail/reply view word-wraps the full note content using wrap_body. [^b48d8-2]

The content_preview function is removed from the codebase. [^b48d8-3]

Chirp must add NostrMinimalContentView from the gallery as a compact flow-layout renderer for previews and search surfaces. [^9a2c7-9]
## Inline Content Concatenation

Consecutive inline segments (text, hashtag, url, emoji, invoice, mention) are grouped and collapsed into a single Text view via concatenation so that hashtags flow inline with surrounding text. Hashtags render as bold #tag text in accent color within the concatenated inline run. Unresolved emojis render as :shortcode: text inline within the concatenated Text run. [^ec51a-2]

## Emoji Rendering

Resolved emojis (with a URL) render as 20×20pt AsyncImage slots using FlowLayout, breaking out of the Text concatenation run. [^ec51a-3]

## FlowLayout

FlowLayout is a reusable Layout implementation used for runs containing resolved emoji images. [^ec51a-4]

## Image Rendering

Chirp's NostrContentRenderer must be synced with the gallery version to include a centralized NostrImageCache (32 MB mem / 256 MB disk) and a pluggable imageLoader closure, replacing raw AsyncImage usage. Apps inject custom image and video loaders via the NmpMediaRenderer environment key on an ancestor view. Images fade in smoothly on load using a FadeInModifier applied to the AsyncImage success case. Images are tappable and open a full-screen viewer with a dark backdrop, close button, and tap-to-dismiss. Posts with multiple consecutive images render all images in the media node, not just the first URL.

<!-- citations: [^ec51a-5] [^19e07-7] [^9a2c7-8] -->
## Video Rendering

Videos render as a card with a Color(.secondarySystemBackground) background instead of a debug label and URL list. Apps inject custom image and video loaders via the NmpMediaRenderer environment key on an ancestor view.

<!-- citations: [^ec51a-6] [^5d893-9] -->
## Mention Rendering

NoteContentView renders nostr: mentions as @npub1… in accent color. NoteRowView.authorDisplayLabel and ModularBlockView.moduleRow must prefer mentionProfiles[pubkey]?.display over item.authorPubkey.shortHex to show the author's display name.

chirp-tui's RenderIntentTracker must extract pubkeys from tokenized content mentions (not just row authors) and emit AuthorProfile intents for them.

<!-- citations: [^86221-4] [^ec51a-7] [^5d893-10] [^9a2c7-7] -->
## Like Button Animation

Tapping the like button in NoteActionsRow applies a spring scale animation with response 0.25 and dampingFraction 0.4. ProfileNoteRow and ThreadNoteRow include the same spring like animation and haptic feedback as the main feed's NoteActionsRow. [^19e07-8]

## Quoted Event Cards

Quoted/embedded event placeholder cards are tappable and navigate to the thread view instead of being dead UI. [^19e07-9]
## See Also

