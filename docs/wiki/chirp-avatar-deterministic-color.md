---
title: Chirp Avatar Deterministic Color
slug: chirp-avatar-deterministic-color
summary: ChirpAvatar uses a deterministic color derived from the first 6 hex characters of the peer's pubkey instead of a flat gray placeholder
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-29
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
---

# Chirp Avatar Deterministic Color

## Deterministic Color Assignment

ChirpAvatar uses a deterministic color derived from the first 6 hex characters of the peer's pubkey instead of a flat gray placeholder. Per-identity colors are derived by djb2-hashing the npub (not the mutable display_name) for stable color assignment. This applies across all row types: DmConversationRow, GroupChatMessageRow, and MarmotMessageRow all use ChirpAvatar with deterministic color instead of flat .quaternary placeholder avatars. ChirpAvatar must call claimProfile(pubkey:) on appear and releaseProfile on disappear so the kernel reactively fetches kind:0 profile data for visible pubkeys.

<!-- citations: [^19e07-1] [^4f377-2] [^9a2c7-2] -->
## See Also

