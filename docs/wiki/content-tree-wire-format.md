---
title: ContentTreeWire — Single Wire Format for Content Rendering
slug: content-tree-wire-format
summary: ContentTreeWire is the single wire format for content rendering across all platforms
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-28
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:1572547f-2b2d-49fb-a383-e95ca25d0bc3
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
---

# ContentTreeWire — Single Wire Format for Content Rendering

## Content Tree Wire Format

ContentTreeWire is the single wire format for content rendering across all platforms. Android converges onto ContentTreeWire, removing the legacy SegmentDto type. nmp-content owns the typed wire for ContentTreeWire/WireNode variants (no ContentRenderData). ContentTreeWire is delivered as a value-type property on TimelineItem within the snapshot itself; there is no separate subscription for it.

<!-- citations: [^15725-6] [^15725-8] [^15725-7] [^56db9-2] [^54ae9-5] -->
## See Also

