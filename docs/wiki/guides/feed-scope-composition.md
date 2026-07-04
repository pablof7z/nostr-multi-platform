---
title: Feed Scope Composition and Active-User Degradation
slug: feed-scope-composition
topic: app-feed
summary: Chirp's home feed uses a Difference(follows, mute) source composition with RootIndexed primary_kinds [1] and All admission for the Android/desktop/TUI Rust shel
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Feed Scope Composition and Active-User Degradation

## Feed Scope Composition

Chirp's home feed uses a Difference(follows, mute) source composition with RootIndexed primary_kinds [1] and All admission for the Android/desktop/TUI Rust shells. The difference composition Difference(ActiveUserFollows, ListMembers(ACTIVE_MUTE_LIST)) hard-errors pre-login with ScopeNotSupportedYet because the mute source is RequireActive, whereas plain ActiveUserFollows degrades gracefully to empty with AllowMissingActive. The iOS home feed uses plain ActiveUserFollows (not the difference composition), built in Swift. The read-model collapse's live relay→feed path is proven sound end-to-end through the real Chirp Rust shell at the device's pinned NMP rev (fa49d00c), including C-ABI, compose, Difference(follows, mute) resolution, decode, and render. The Chirp home feed renders on a real Android emulator with 3 cards from a followed author's live notes, device-verified after re-pinning to the fixed NMP.

<!-- citations: [^dcc80-2695e] [^dcc80-ad5c2] [^dcc80-5431e] -->
