---
title: Embed Inline-Flow Rendering — Cards Within Surrounding Prose
slug: embed-inline-flow-rendering
summary: Event embeds must render inline within their surrounding note text on every platform — the card sits between preceding and following prose, never swallowing it or showing raw URIs.
tags:
  - components
  - rendering
  - embed
  - nmp-gallery
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-28
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:ec51ad49-af31-4415-aab4-e9123eb63eab
  - session:29d2c220-a86b-4b0d-82fb-d40d8fd4505e
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:c5325e71-7d4e-451e-8c15-81cdae440f5f
  - session:1572547f-2b2d-49fb-a383-e95ca25d0bc3
  - session:fa300009-e498-4c80-a2d3-64d1531a09d4
  - session:e64f6909-2f82-4eae-b46d-0074b7c4d711
  - session:9de494e6-e783-4785-ae67-1f7014dadd5d
---

# Embed Inline-Flow Rendering — Cards Within Surrounding Prose

> Event embeds must render inline within their surrounding note text on every platform — the card sits between preceding and following prose, never swallowing it or showing raw URIs.

## Core Requirement

Content rendering uses a single recursive engine so that an embedded event's content renders identically to a top-level event of the same kind, with the embed shell providing only visual chrome (indent, border, depth cue). Every event embed (article, note, profile, highlight) must render inline within its surrounding note text on every platform. All event embeds must show the referenced event. The canonical pattern: surrounding prose text → embedded card → continuation of surrounding prose. For example, if the note content is 'hey, check out my article nostr:naddr1... I hope you like it!', the rendered output must show 'hey, check out my article' followed by the medium-like article card followed by 'I hope you like it!'. The embed card must not swallow or hide the surrounding prose, and the raw nostr: URI must never appear as visible text.

Hashtags must render inline with surrounding text rather than occupying their own block row.

Images and videos must render inline with actual visual views instead of debug label+URL lists.

<!-- citations: [^6a951-82] [^6a951-23] [^6a951-91] [^ec51a-1] [^6a951-25] [^6a951-34] [^6a951-93] [^ec51a-2] [^c5325-2] [^15725-8] -->
## Failure Conditions

The verification FAILS if any of these conditions hold: (1) The raw nostr:naddr1…/nostr:nevent1… URI appears as visible text instead of being replaced by the embed card. (2) The embed card renders but the surrounding prose text is missing — the card swallowed the content around it. (3) The embed card renders but the surrounding prose is displayed as a separate block rather than flowing inline with the card. (4) The embed card shows stale placeholder text ('Loading embedded event…', 'Fetching from relay…') as its final rendered state rather than eventually resolving to real event data. [^6a951-83]

## Per-Embed-Type Verification

For article embeds: the card must show author display name (not pubkey), article title, summary, hero image (actually rendered, not a placeholder), and must sit between the preceding and following prose. ContentView renders embedded events as inline embed cards at the exact EventRef position within the content tree. ContentView dispatches inline embeds by kind: Article to ArticleCard, ShortNote to author + content preview, Highlight to highlighted text + source link, and Profile to display name chip. ArticleCard uses a horizontal layout when a hero_image_url is present, with an 80×80px image on the left and title, byline, and summary on the right; it falls back to a vertical stacked layout when no hero image is present. ArticleCard images are fetched via ureq, decoded with the image crate, and cached as egui textures. The article embed component renders a bold title with +1 left inset to separate it from surrounding post text, displays a byline in the format ● Author · Date · Reading Time (red dot, light author, dim date and read time), includes an estimated reading time indication, displays the summary in muted grey, and has a fixed height of 5 lines. Android NostrContentView dispatches kind:30023 (article) to a typed NostrArticleCard with hero image, title, summary, and byline, instead of routing all inline EventRefs through a generic NostrQuoteCard. The article embed component uses a rounded ╭─╮ box around the card. For note embeds: the card must show the quoted note's author display name and content, surrounded by the embedding note's prose. The Android NostrQuoteCard renders created_at as a display string provided by the page's hydration code; the fix is to format the raw epoch into relative time at the hydration sites, not inside the card component. Embedded quoted event cards are tappable and navigate to the thread instead of showing dead placeholder text. For profile embeds: the card must show the profile's display name, avatar, and NIP-05, surrounded by prose. For highlight embeds: the pull-quote text and context must render, surrounded by prose.

Embedded event widgets on TUI, iOS, and Android must render the embedded event with a clearly visible full box to distinguish it as an event inside another event. The TUI embedded event widget uses Borders::ALL instead of Borders::LEFT to draw a full rectangular box around the embedded event, adds 2 rows to preferred_height for top/bottom borders, and recalculates inner width by subtracting 2 for borders and 2 for body indentation. The iOS embedded event widget uses a border lineWidth of 1.5 instead of 0.5 to make the box clearly visible. The Android embedded event widget uses a border width of 1.5.dp instead of 0.5.dp for all three card variants (collapsed, frame, missing) to make the box clearly visible.

Event, article, and address references must render as block cards (NostrEntityCard) on iOS and bordered placeholder cards on Android.

<!-- citations: [^6a951-84] [^6a951-10] [^6a951-49] [^29d2c-6] [^6a951-122] [^19e07-12] [^fa300-1] [^e64f6-1] [^9de49-1] -->
## Platform Scope

This requirement applies identically to all four platforms: iOS, Android, TUI, and Desktop. Every embed component on every platform is subject to the inline-flow verification in the 64-cell matrix at docs/testing/nmp-gallery-verification-matrix.md.

Android has a confirmed inline-flow gap: the EmbedComponentPages.kt renders each embed as a bare card in a Column, wrapped in meta-prose (a description label + footnote), but does not compose the actual surrounding note text inline (e.g., 'this is a great point ' [card] ' what do you think?') the way iOS composes a content tree. The card sits isolated with no real surrounding prose — the showcase reference text strings exist in the data but are never rendered around the card on Android. Android embed pages must compose text + eventRef + text inline via NostrContentView (not a bare EventDisplayCard), so surrounding note text renders around the embedded card.

<!-- citations: [^6a951-85] [^6a951-94] [^6a951-121] -->
## Inline Segment Composition

Consecutive inline segment types (text, hashtag, url, emoji, invoice, mention) must be grouped and collapsed into a single concatenated Text view. [^6a951-33] [^6a951-91] [^ec51a-3]

The SegmentDtoView composable must support inline text, hashtags, URLs, mentions, emoji, and invoices, plus block media, event refs, and markdown. [^ec51a-10]

Hashtags must render as bold #tag in accent color within the concatenated text run. [^6a951-91] [^ec51a-4]

Resolved emojis (with a URL) must render as 20×20pt inline image slots using a FlowLayout when present alongside text, falling back from Text concatenation. [^6a951-56] [^6a951-104] [^ec51a-5]

Unresolved emojis must render as :shortcode: text inline within the concatenated Text run. [^6a951-104] [^ec51a-6]

Videos must render as a dark card displaying the filename. [^6a951-34] [^6a951-93] [^ec51a-7]

FlowLayout is a reusable Layout implementation for wrapping inline content. [^6a951-104] [^ec51a-8]

<!-- citations: [^6a951-33] [^6a951-91] [^ec51a-3] [^ec51a-4] [^6a951-56] [^6a951-104] [^ec51a-5] [^ec51a-6] [^6a951-34] [^6a951-93] [^ec51a-7] [^ec51a-8] [^29d2c-5] -->
## Chirp NoteContentView Tokenization

Chirp's NoteRowView, ThreadNoteRow, and ProfileNoteRow must use NoteContentView instead of rendering item.content as raw Text. [^6a951-119] [^6a951-125] [^6a951-126] [^6a951-148] [^ec51a-9]

NoteContentView must tokenize content client-side using regex on nostr:, https://, and #tag patterns. [^6a951-147] [^ec51a-10]

In Chirp's NoteContentView, #tag patterns must render as inline hashtags in accent color. [^6a951-147] [^ec51a-11]

In Chirp's NoteContentView, nostr: mentions must render as @npub1… in indigo. [^6a951-147] [^ec51a-12]


NostrRichText must be wired into the TimelineScreen, replacing the raw Text() call. [^6a951-13] [^6a951-50] [^29d2c-9]
## Embed Depth and Cycle Guards

EmbedCard must implement PD-015 depth and cycle guards as an exact port of the iOS RenderContext logic. [^6a951-40] [^29d2c-7]

## Markdown Block Handling

MarkdownNodeView and InlineFlow must handle CommonMark blocks including headings, paragraphs, lists, blockquotes, code, and rules. [^6a951-41] [^29d2c-8]

## Embed Shell Separation

The EmbedChromeContainer (F-CR-08) ships inside F-CR-05/06/07 and knows nothing about content, providing only border, indent, and depth cues. [^15725-9]
## See Also
- [[nmp-gallery-verification-matrix|NMP Gallery Verification Matrix — 64-Cell Cross-Platform Quality Gate]] — related guide
- [[nmpui-website|nmpui.f7z.io — Component Showcase Website]] — related guide

