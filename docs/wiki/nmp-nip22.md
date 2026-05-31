---
title: "NMP NIP-22 Crate: CommentRecord & Comment Pointers"
slug: nmp-nip22
summary: The `nmp-nip22` crate provides `CommentRecord` with `CommentPointer` variants (Event, Address, External) for both root (uppercase tags) and parent (lowercase)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:423f3c56-7275-4e62-998e-e8f37be564da
---

# NMP NIP-22 Crate: CommentRecord & Comment Pointers

## Comment Model and Builders

The nmp-nip22 crate provides CommentRecord with CommentPointer variants (Event, Address, External) for both root (uppercase tags) and parent (lowercase). Builder entrypoints include Comment::on_event, Comment::on_address, and Comment::on_external, with .reply_to_comment(...) for nesting. CommentPointer aliases nmp-threading::ThreadPointer with byte-identical serde.

<!-- citations: [^590ca-6] [^423f3-10] -->
## See Also

