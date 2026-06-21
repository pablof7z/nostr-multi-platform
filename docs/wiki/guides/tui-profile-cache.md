---
title: TUI Profile Cache
slug: tui-profile-cache
topic: tui
summary: The TUI must maintain a client-side profile cache mapping pubkey to display_name, picture_url, nip05, and about, since timeline snapshots contain only hex pubke
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-25
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
---

# TUI Profile Cache

## Client-Side Profile Cache

The TUI must maintain a client-side profile cache mapping pubkey to display_name, picture_url, nip05, and about, since timeline snapshots contain only hex pubkeys. The chirp-tui RenderIntentTracker emits AuthorProfile intents for both note authors and content-mention pubkeys, driving nmp_app_claim_profile/release_profile calls.

<!-- citations: [^4f377-23] [^86221-10] -->
## Reaction, Repost, and Reply Counts

Reaction counts, repost counts, and reply counts are not available in the timeline snapshot and must be aggregated via a separate domain query against the kernel's nmp-reactions domain. <!-- [^4f377-24] -->
