---
title: ContentTreeWire Format & Fixtures
slug: content-tree-wire
summary: The ContentTreeWire fixture set covers text (S-T01–10), mentions (S-M01–03), quotes (S-M04–09), lists, media (S-MD01–03), links (S-L01–03), hashtags (S-H01–03),
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
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:cd331450-f93f-48d0-960e-3c73e927775e
---

# ContentTreeWire Format & Fixtures

## Fixture Scope

The ContentTreeWire fixture set covers text (S-T01–10), mentions (S-M01–03), quotes (S-M04–09), lists, media (S-MD01–03), links (S-L01–03), hashtags (S-H01–03), and fallback/edge cases (S-E01–07). Wire contract golden fixture JSONs must match what `cargo run -p nmp-content-fixtures --bin build-wire-fixtures` generates; the `wire_goldens_match` test enforces this. [^45258-5] [^523-524]

The nmp-content crate owns the typed wire for ContentTreeWire and WireNode variants. [^56db9-1]

The iOS NFCT content-tree decoder handles all 22 WireNodeKind variants (text, mention, eventref, hashtag, url, media, emoji, invoice, heading, paragraph, blockquote, codeblock, list, rule, emphasis, strong, inlinecode, link, image, softbreak, hardbreak, placeholder) plus mode and nostr-uri sub-enums. V-84 (iOS typed NFCT content-tree decoder) is merged via PR #762. [^762-100]

<!-- citations: [^45258-5] [^523-524] [^56db9-1] [^45258-6] [^f2605-5] [^cd331-3] -->
## Wire Format Degradation Rules

Markdown links with bad hrefs degrade to text runs rather than `Link { href: None }` in the ContentTreeWire wire format. Lightning and cashu tokens do not yet lift to `Segment::Invoice` in the ContentTreeWire wire format. [^45258-7]


The nmp-content crate does not include ContentRenderData in its typed FlatBuffers wire. In the NFTS schema, content_render_bytes carries serde_json bytes as a known gap until a typed encoder for ContentRenderData is implemented in a follow-up. [^56db9-2]

ContentTreeWire is delivered as a value-type property on TimelineItem within the snapshot itself, with no separate subscription. [^54ae9-2]
## Bare Inline Root Wrapping

Bare inline `NostrWireNode` roots at the top of `ContentTreeWire` must be wrapped in `.paragraph(children: [...])` for proper inline flow, otherwise `nostrContentGroups` adds a newline sentinel after each bare inline node. [^45258-8]
## See Also

