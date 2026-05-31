---
title: "NMP NIP-01 Crate: NoteRecord, Replies & Threads"
slug: nmp-nip01
summary: "The `nmp-nip01` crate provides `NoteRecord`, a `Note::new(...).reply_to(parent).build(...)` builder with NIP-10 marked root/reply markers and parent-author-firs"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-28
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
---

# NMP NIP-01 Crate: NoteRecord, Replies & Threads

## NoteRecord and Thread Views

The `nmp-nip01` crate provides `NoteRecord`, a `Note::new(...).reply_to(parent).build(...)` builder with NIP-10 marked markers and parent-author-first p-tag dedup. Reply NIP-10 tagging uses the reply marker only, with no root forwarding and no p re-notification. `RepliesView` provides flat direct replies, and `ThreadView` provides parent/child trees with out-of-order arrival buffering. The crate also owns the typed wire for `TimelineEventCard`, `TimelineBlock`, and `ModularTimelineSnapshot`, as well as `ContentRenderData`, and represents `NoteRelationCounts` as an enum. Visible note relations claims are forwarded as JSON actions on namespace `nmp.nip01.visible_note_relations` with an auto-generated `consumer_id` for refcounting into the Rust working set.

<!-- citations: [^590ca-5] [^423f3-9] [^57528-14] [^56db9-10] [^54ae9-20] -->
## See Also

