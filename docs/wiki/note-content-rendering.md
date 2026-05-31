---
title: Note Content Rendering
slug: note-content-rendering
summary: TimelineRow stores the full, untruncated note content rather than a capped preview
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-28
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:b48d81e1-411c-45db-a440-340bcaee2631
  - session:a889fe39-a56b-4ba4-8fc2-4c202a3ecfbe
  - session:d366b3c7-f7a7-49d5-9961-625037c7deb6
---

# Note Content Rendering

## Note Content Rendering

TimelineRow stores a `raw_card: String` field containing the canonical Nostr wire-format JSON (id, pubkey, kind, created_at, content, tags, sig) for the event. The timeline list view truncates content to the terminal width at render time rather than relying on pre-truncated stored data. The detail/reply view word-wraps the full note content rather than operating on truncated data. Inline note content uses native `Row::wrap()` instead of `iced_aw`.

<!-- citations: [^b48d8-1] [^a889f-6] [^d366b-6] -->
## See Also

