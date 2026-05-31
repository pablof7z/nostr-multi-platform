---
title: Home Timeline Seed Bootstrap
slug: home-timeline-seed-bootstrap
summary: The home-timeline seed content modules (seed-bootstrap, seed-contacts, seed-profiles, seed-relays) currently rely on hardcoded pubkeys and require a separate ki
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-19
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:fc128f85-af57-41cd-8c5b-a71d15450e17
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:5d180e52-7c43-4a99-bfc4-769eb40dc03f
---

# Home Timeline Seed Bootstrap

## Home Timeline Seed Bootstrap

The seed timeline (hardcoded accounts jb55, fiatjaf, pablo) is removed. On startup, the app fetches the active user's own kind:3, profile, and relay list instead of emitting hardcoded seed bootstrap requests. The timeline authors set includes the active user's own pubkey so they see their own posts, and excludes hardcoded seed pubkeys. The timeline opened milestone gates on the active account's contacts being available, not on hardcoded seed contact lists. The subscription ID prefix for the follow timeline uses 'follow-timeline-' (renamed from 'seed-timeline-'). The status system reports 'Timeline' instead of 'SeedTimeline(fiatjaf,jb55,pablof7z)'. Batched discovery requests for unknown pubkeys fetch kinds 0, 3, and 10002.

<!-- citations: [^09da8-3] [^fc128-2] [^fd809-2] [^5d180-1] -->
## See Also

