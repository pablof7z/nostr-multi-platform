---
title: Chirp Profile Resolution and Change Publisher Subscription
slug: chirp-profile-resolution
topic: ui-components
summary: Profile resolution failing â rows showing raw hex pubkeys instead of names and avatars â is a core bug, not a cosmetic issue
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

# Chirp Profile Resolution and Change Publisher Subscription

## Profile Resolution

Profile resolution failing — rows showing raw hex pubkeys instead of names and avatars — is a core bug, not a cosmetic issue. Profile resolution on both iOS and Android subscribes to the change publisher rather than reading profiles synchronously, fixing a race where rows showed raw hex pubkeys instead of names and avatars.

On iOS, the fix added profile-change subscription to six row sites that were reading the profile synchronously but never subscribed to the change publisher.

On Android, the fix corrected a race in `KernelProfileHost` where a collector attaching after the single no-replay event fired missed the profile data.

A related per-row profile-cache/dedup quirk remains open as chirp#46: two of three rendered feed rows show the author's raw shortHex pubkey instead of the resolved display name.

<!-- citations: [^dcc80-04d80] [^dcc80-26a10] [^dcc80-818db] [^dcc80-76200] [^dcc80-d0bfc] -->
