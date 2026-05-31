---
title: Chirp Thread Fetching & Hydration
slug: chirp-thread-fetching
summary: Opening a thread always fires REQs to relays for the focused event, its root, and tagged replies unconditionally, with no cache hit check to skip fetching alrea
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:cc7dc68a-1fcd-49fe-98be-198f17b6d59e
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Chirp Thread Fetching & Hydration

## Thread Fetching Behavior

Opening a thread always fires REQs to relays for the focused event, its root, and tagged replies unconditionally, with no cache hit check to skip fetching already-known events. The self.events cache is used only for building the UI, not for deciding whether to skip fetching. The thread-ids REQ uses a limit of 20 and batches pending IDs in groups of 20, re-driving hydration after each relay frame. The thread-replies REQ fetches kinds [1, 6] with a limit of 200. ThreadViewState.reply_kinds must store the reply kinds directly rather than only parameterizing them in the thread REQ, because the deferred-relay hydration fires on a later tick where the original kind context would otherwise be lost.

<!-- citations: [^cc7dc-2] [^4edd4-4] -->
## See Also

