---
title: NWC Wallet RelayRole — Bootstrap Gate Exclusion and Lazy Initialization
slug: nwc-wallet-relay-role
summary: "RelayRole::Wallet is excluded from the bootstrap startup gate and RelayRole::all(), ensuring it does not block app startup."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-19
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:274d6f3c-5974-48a6-a985-570ae0ae805d
  - session:fc128f85-af57-41cd-8c5b-a71d15450e17
  - session:87fd49fb-4869-4c40-9a6a-96545bd2313d
---

# NWC Wallet RelayRole — Bootstrap Gate Exclusion and Lazy Initialization

## Bootstrap & Startup Exclusion

RelayRole::Wallet is excluded from the bootstrap startup gate and RelayRole::all(), ensuring it does not block app startup. The all_relays_connected gate must be reworked to gate on Indexer-only at startup, with Content relays joining on-demand, to prevent cold start deadlocks when dropping the Content lane bootstrap.

<!-- citations: [^274d6-9] [^fc128-4] [^87fd4-3] -->
## Lazy Initialization

RelayRole::Wallet relay health is lazily initialized via entry().or_default() in relay_mut. spawn_missing_relays spawns only Indexer workers from the app-provided list with no Content bootstrap.

<!-- citations: [^274d6-10] [^fc128-5] [^87fd4-4] -->
## Relay Role Model

Relay roles are additive — any combination of Read, Write, Indexer, and Wallet can be assigned to a single relay. Indexer and Wallet (app relays) are configurable relay roles. [^87fd4-1]

In the Rust backend, relay role is stored as a space-separated capability list (e.g. 'indexer read write'), parsed and sorted by normalize_roles(). normalize_roles() provides backward compatibility for legacy 'both' entries. The has_role() function checks whether a relay possesses a specific capability. [^87fd4-2]
## See Also

