---
title: NIP-01 — Notes, Threads, and Replies (nmp-nip01 Crate)
slug: nmp-nip01
summary: "NMP provides an nmp-nip01 crate featuring NoteRecord (an immutable decoded struct), a Note::new(...).reply_to(parent).build(...) builder that produces NIP-10 ma"
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
  - session:b6578d9e-697f-41ae-ab75-5e5643ceff13
  - session:6e6bcf78-bf6b-4ddd-a2b8-4fb829d86604
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
---

# NIP-01 — Notes, Threads, and Replies (nmp-nip01 Crate)

## nmp-nip01 Crate

NMP provides an nmp-nip01 crate featuring NoteRecord (an immutable decoded struct; published notes are Nostr kind:1 notes), a Note::new(...).reply_to(parent).build(...) builder that produces NIP-10 marked-form root/reply markers, Nip10Resolver (where Nip10Resolver::supersedes returns the target event ID for kind:6 reposts by decoding via nmp_nip18), RepliesView, ThreadView (with out-of-order arrival buffering), RepliesDomain, the typed wire for TimelineEventCard, TimelineBlock, and ModularTimelineSnapshot, ContentRenderData (with NoteRelationCounts as enum), and ClaimVisibleNoteRelations and releaseVisibleNoteRelations dispatched as JSON actions on namespace nmp.nip01.visible_note_relations with an auto-generated consumer_id, performing refcounting into the Rust working set. The NoteRecord domain type is implemented in the nostr-broadcast-core crate.

<!-- citations: [^590ca-6] [^b6578-6] [^6e6bc-10] [^56db9-6] [^54ae9-13] -->
## See Also

