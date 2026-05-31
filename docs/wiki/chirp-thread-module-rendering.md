---
title: Chirp Thread Module Rendering
slug: chirp-thread-module-rendering
summary: When two reply events (e.g., event0 and event1) appear nearby in the timeline, they display as a stacked module connected by a vertical line, similar to Twitter
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:cc7dc68a-1fcd-49fe-98be-198f17b6d59e
---

# Chirp Thread Module Rendering

## Thread Module Rendering

When two reply events (e.g., event0 and event1) appear nearby in the timeline, they display as a stacked module connected by a vertical line, similar to Twitter's threading UX. Vertical connecting lines in threaded modules are drawn through the avatar column center, not at the screen edge. Avatars in modules render as full-size properly stacked rows, not tiny overlapping circles. Text from stacked events in modules uses proper row containers so rows do not bleed into or overlap each other. A Show this thread pill is displayed below each threaded module. ChirpRoute.thread accepts an optional TimelineItem as an initialItem associated value. When ThreadScreen has a nil threadView but a non-nil initialItem, it renders the focused note immediately using ThreadNoteRow(isFocused: true) with a small spinner below it instead of the fetching-from-relays placeholder. NoteRowView, ProfileView, and ModularBlockView pass the TimelineItem they have in scope as initialItem when pushing a thread route. SearchView and the ModularBlockView 'Show this thread' pill pass nil as initialItem when pushing a thread route because no item is available at those call sites.

<!-- citations: [^423f3-3] [^cc7dc-3] -->
## See Also

