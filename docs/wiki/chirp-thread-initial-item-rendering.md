---
title: Chirp Thread Initial Item Rendering
slug: chirp-thread-initial-item-rendering
summary: When a user taps an event to open a thread, the ThreadScreen must render the tapped TimelineItem immediately from the route payload rather than showing a 'fetch
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:cc7dc68a-1fcd-49fe-98be-198f17b6d59e
---

# Chirp Thread Initial Item Rendering

## Immediate Initial-Item Rendering

When a user taps an event to open a thread, the ThreadScreen must render the tapped TimelineItem immediately from the route payload rather than showing a 'fetching events' placeholder while waiting for the kernel snapshot. [^cc7dc-1]


The ChirpRoute.thread case includes an initialItem associated value of type TimelineItem? to carry the tapped item into ThreadScreen. [^cc7dc-2]

NoteRowView, ProfileView, and ModularBlockView pass their in-scope TimelineItem as the initialItem when pushing a thread route. SearchView and ModularBlockView's 'Show this thread' pill pass nil as initialItem because no TimelineItem is available at those call sites. [^cc7dc-3]

When model.threadView is nil and initialItem is non-nil, ThreadScreen renders the focused note immediately using ThreadNoteRow(isFocused: true) with a small spinner below it, instead of the 'fetching from relays' placeholder. [^cc7dc-4]
## See Also

