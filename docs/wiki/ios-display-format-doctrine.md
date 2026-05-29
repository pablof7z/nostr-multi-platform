---
title: iOS Display Format Doctrine (ADR-0032) — Raw-Data Projection Bridge
slug: ios-display-format-doctrine
summary: ADR-0032 requires all pre-formatted strings to be removed from Bridge Decodables; PubkeyFormatting.swift exists with the right helpers but ~5 groups of structs still expose pre-formatted fields.
tags:
  - ios
  - swift
  - adr-0032
  - bridge
  - v53
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# iOS Display Format Doctrine (ADR-0032) — Raw-Data Projection Bridge

> ADR-0032 requires all pre-formatted strings to be removed from Bridge Decodables; PubkeyFormatting.swift exists with the right helpers but ~5 groups of structs still expose pre-formatted fields.

## Existing Helpers (PubkeyFormatting.swift)

Presentation-layer formatting is implemented in `ios/Chirp/Chirp/Extensions/PubkeyFormatting.swift`:
- `String.shortHex` — abbreviated pubkey (first 8 + last 8 chars with ellipsis)
- `String.pubkeyColor` — deterministic SwiftUI Color from pubkey hash
- `String.pubkeyColorHex` — hex avatar tint
- `String.displayInitials` — 2-char initials for avatars
- `UInt64.relativeTimeFromUnixSeconds` — relative-time labels

A `DisplayFormat.swift` wrapper/namespace does not yet exist. [^42908-19]

## V-53: Pre-formatted Fields Still Present

ADR-0032 (raw-data projection doctrine) requires all pre-formatted strings to be dropped from Bridge Decodables. The following still carry pre-formatted fields:

- `RelayDiagnosticsWireSub` and `RelayDiagnosticsRow`: `stateLabel`, `consumerCountLabel`, `eventsRxDisplay`, `openedDisplay`, `lastEventDisplay`, `eoseDisplay`, `roleLabel`, `connectionLabel`, `authLabel`, `totalEventsDisplay`, `bytesRxDisplay`, `bytesTxDisplay`, `lastConnectedDisplay`
- `ThreadView`: `previousCountLabel`, `nextCountLabel`
- `PublishOutboxItem`: `createdAtDisplay`, `statusLabel`, `systemImage`, `canRetry`, `targetSummary`
- `PublishOutboxRelay`: `statusLabel`, `attemptLabel`, `relayReason`
- `BunkerHandshake`: `stageLabel` + status-computed flags

Core timeline and DM structs are clean. [^42908-20]

## Required Work (V-53)

1. Create `DisplayFormat.swift` as a namespace wrapper consolidating existing helpers
2. Drop all pre-formatted `*Display`, `*Label`, `*Tone` fields from Bridge Decodables with explicit `CodingKeys` [^42908-21]

## See Also

