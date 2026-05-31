---
title: V-42 — NIP-51 Mute List (HIGH · v1-A Safety)
slug: v-42-nip51-mute-list
summary: "V-42 NIP-51 mute list (HIGH v1-A safety): substrate-generic SuppressionLookup, read-time owner gate, cross-account correctness, PR #834."
tags:
  - backlog
  - V-42
  - NIP-51
  - mute
  - v1-safety
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# V-42 — NIP-51 Mute List (HIGH · v1-A Safety)

> V-42 NIP-51 mute list (HIGH v1-A safety): substrate-generic SuppressionLookup, read-time owner gate, cross-account correctness, PR #834.

## Overview

V-42 is a HIGH-priority v1-A safety backlog item: implement NIP-51 mute list support. The feature allows users to mute (suppress timeline content from) specific pubkeys per NIP-51. The implementation uses a substrate-generic SuppressionLookup trait (D0-clean, mirrors BlockedRelayLookup), with composition-time injection so nmp-nip01 avoids a forbidden sibling dep. Suppression is applied at ingest-time and snapshot-time with an active-account gate. The implementation surfaced the nmp-wot kind:10000 overlap rather than silently duplicating it. [^4edd4-145]

## Review — BLOCKED Then Approved

The first review was PARTIAL — D0, layer-boundary, active-account-gate, replaceable semantics, and tests all PASS, but two issues were found: (1) hard CI block — nmp-nip51 missing from release/nmp-release.toml; (2) a real account-switch stale-mute gap — between switching accounts and the new account's kind:10000 arriving, the prior account's mutes stay active (the test masked it by sending the new list immediately). The second is a genuine correctness bug — account B should never inherit account A's mutes, same principle as the active-account gate. [^4edd4-146]

## Fix — Read-Time Owner Gate

The fix agent chose the better pattern: a read-time owner gate (like FollowListProjection) instead of a composition-root hook that would've needed wiring. Production is fixed unconditionally — the active account is checked at mute-read time, so switching accounts immediately drops old mutes. The strengthened test proves the cross-account leak is closed. The re-review confirmed both fixes (release manifest + owner gate) and the PR was approved and merged as PR #834 (master at 21802721). [^4edd4-147]

## Architectural Decisions

SuppressionLookup is substrate-generic (D0-clean). It mirrors the BlockedRelayLookup pattern. Composition-time injection avoids nmp-nip01 depending on nmp-nip51 (forbidden sibling dep). Replaceable-event semantics are used for kind:10000 mute lists. The owner gate is a genuine live re-read with no caching, ensuring cross-account correctness. Suppression is applied at ingest-time and snapshot-time with an active-account gate.

<!-- citations: [^4edd4-148] [^4edd4-236] -->
## See Also

