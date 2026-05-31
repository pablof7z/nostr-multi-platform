---
title: NIP-51 Mute Suppression & Read-Time Owner Gate
slug: nip-51-mute-suppression
summary: V-42 NIP-51 mute suppression uses a read-time owner gate (like FollowListProjection) rather than a composition-root hook, so production is fixed unconditionally
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# NIP-51 Mute Suppression & Read-Time Owner Gate

## Mute Suppression Design

V-42 NIP-51 mute suppression uses a read-time owner gate (like FollowListProjection) rather than a composition-root hook, so production is fixed unconditionally without needing wiring. Between accounts, the prior account's mutes must not stay active; the cross-account mute leak is closed by the read-time owner gate. [^4edd4-15]

## See Also

