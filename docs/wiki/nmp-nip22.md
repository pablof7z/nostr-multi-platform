---
title: NIP-22 — Comments (nmp-nip22 Crate)
slug: nmp-nip22
summary: "NMP provides an nmp-nip22 crate for standalone kind-1111 comments with CommentRecord, CommentPointer (Event/Address/External), Comment::on_event/on_address/on_e"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-27
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:9e632bcb-fecc-4cda-a228-9a09e8db07ed
---

# NIP-22 — Comments (nmp-nip22 Crate)

## nmp-nip22

NMP provides an nmp-nip22 crate for standalone kind-1111 comments with CommentRecord, CommentPointer (Event/Address/External), Comment::on_event/on_address/on_external builder entrypoints with .reply_to_comment() nesting, CommentsView, and CommentsDomain. NIP-22 (kind:1111) comment support is post-v1; the engine and resolvers are designed so a future Nip22Resolver reuses RootIndexedFeed with zero new infrastructure.

<!-- citations: [^590ca-7] [^9e632-6] -->
## See Also

