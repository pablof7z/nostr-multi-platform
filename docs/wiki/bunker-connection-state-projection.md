---
title: BunkerConnectionState — NIP-46 Session Visibility Projection
slug: bunker-connection-state-projection
summary: A `BunkerConnectionState` projection must be exposed to the host so that NIP-46 relay-flap session drops are visible rather than silently bricking the client.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-18
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
---

# BunkerConnectionState — NIP-46 Session Visibility Projection

## Bunker Connection State Projection

A `BunkerConnectionState` projection must be exposed to the host so that NIP-46 relay-flap session drops are visible rather than silently bricking the client. The NIP-46+NIP-42 credential cache silently clears at `identity.rs:237-256`.

<!-- citations: [^4edd4-216] [^57528-2] -->
## See Also

