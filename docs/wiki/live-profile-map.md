---
title: Live Profile Map & Reactive Profile Resolution
slug: live-profile-map
summary: Profile reactivity is handled by a shared LiveProfileMap rather than per-app snapshot extraction boilerplate
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:8bd548b9-af6d-4108-bc64-16ebf8dfa4f7
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:f5503f3a-d44c-4626-b8de-0492ad1f2a6c
  - session:9b9db91a-b324-4c11-aacf-421d9aab2819
  - session:47882225-939f-4978-bf5a-8feb9e5ef029
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Live Profile Map & Reactive Profile Resolution

## LiveProfileMap

The LiveProfileMap reactive store merges three projections: claimed_profiles for inline-mention claims, mention_profiles for timeline authors, and author_view.profile for full cards. Profile reactivity is handled by this shared LiveProfileMap rather than per-app snapshot extraction boilerplate. LiveProfileMap is updated from the snapshot via a single call per tick: live_profiles.update_from_snapshot(snapshot). Its resolve(pubkey) method falls back to a truncated npub if a kind:0 event has not yet arrived. No event should trigger a kernel request for the author's kind:0 profile; profile claiming is always the concern of the presentation layer. Individual screens do not manually call `claim_profile`, `release_profile`, `claim_event`, or equivalent lifecycle APIs just because they placed a component on screen; the shell wires one registry host adapter and components own their lifecycle internally. PR #734's `render.rs` must use `LiveProfileMap` instead of the removed `data.primary_profile` field to compile against the current master.

<!-- citations: [^8bd54-1] [^54ae9-9] [^f5503-3] [^47882-2] [^752b5-4] [^6a951-9] -->
## Render Chain Integration

EmbedFrameContext carries a reference to LiveProfileMap so the render chain threads it to every component call. At render time, profiles.resolve(pubkey) returns a ProfileWire with a truncated npub fallback until kind:0 data arrives from relays. The reactive profile mention feature (§5.4 / M16-C7) displays an honest @npub1abc… placeholder that hydrates to @DisplayName when a kind:0 event arrives.

<!-- citations: [^8bd54-2] [^752b5-5] -->
## Data Model Constraints

GalleryData holds primary_pubkey as a String identity, not a pre-baked ProfileWire. No fake placeholder names like 'Primary Author' or fake pubkeys are used for profile display. Mention profiles in content examples must be specified only by pubkey (or URI), not by baked-in LiveProfile structs with pre-resolved display names. Profile resolution for mention profiles must go through LiveProfileMap at render time rather than being embedded in ContentRenderData upfront. mention_profiles_from_items maps only item.author_pubkey (top-level timeline authors), not pubkeys mentioned inside note bodies.

<!-- citations: [^8bd54-3] [^9b9db-1] [^752b5-6] -->
## Test Boundaries

render_test_data() is gated behind #[cfg(test)] only and not used in live operation. [^8bd54-4]
## See Also

