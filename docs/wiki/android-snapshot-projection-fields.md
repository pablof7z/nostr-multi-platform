---
title: Android Snapshot Projection Fields — dm_inbox, wallet_status, and Profile Views
slug: android-snapshot-projection-fields
summary: Android `decodeProjections()` must extract `dm_inbox` and `wallet_status` from snapshot projections so DM and Wallet screens are not permanently empty.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Android Snapshot Projection Fields — dm_inbox, wallet_status, and Profile Views

## Snapshot Projection Decoding

Android `decodeProjections()` must extract `dm_inbox` and `wallet_status` from snapshot projections so DM and Wallet screens are not permanently empty. [^f3d8d-3]


Android `Snapshot.kt` must include `claimedProfiles`, `mentionProfiles`, and `authorView` fields so `ProfileScreen` shows real data instead of hardcoded placeholder text. [^f3d8d-4]

Android `DmScreen` must call `claimProfile` and `DmConversationListScreen` must accept a `model` parameter so peer names load correctly. [^f3d8d-5]
## See Also

