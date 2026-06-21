---
title: Thread Screen
slug: thread-screen
topic: ui-components
summary: ChirpRoute.thread carries an optional initialItem parameter of type TimelineItem alongside the eventID
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-21
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:cc7dc68a-1fcd-49fe-98be-198f17b6d59e
  - session:17ef19cd-8549-4fa9-b09c-5266aaf480a7
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
---

# Thread Screen

## Thread Screen

ChirpRoute.thread carries an optional initialItem parameter of type TimelineItem alongside the eventID. ThreadScreen renders the focused note immediately from the passed initialItem when the kernel snapshot has not yet arrived, showing a small spinner below it instead of the 'fetching from relays' placeholder. Call sites that already have a TimelineItem in scope (NoteRowView, ProfileView, ModularBlockView) pass it as initialItem; call sites without one (SearchView, ModularBlockView's 'Show this thread' pill) pass nil.

Tapping a kind:6 repost navigates to the inner note's thread (the original note), not to the wrapper kind:6 event's thread. The thread screen (ThreadNoteRow) previously had no heuristic for reposts at all, causing every repost to render as raw JSON.

Embedded or quoted event cards that are unavailable or at a depth greater than 0 are tappable and navigate to the thread instead of being inert.

Thread-opening REQs to relays remain unconditional even when the events are already cached — the kernel always fetches the focused event, its root, referenced events, and all replies. The thread-ids REQ batches pending IDs with a limit of 20 per batch, and maybe_open_thread_hydration is re-driven after every relay frame.

<!-- citations: [^cc7dc-2] [^cc7dc-3] [^17ef1-2] [^19e07-10] -->
