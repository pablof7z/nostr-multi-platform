---
title: Zap Protocol
slug: zap-protocol
topic: zap-scope
summary: The proof-of-payment (preimage) must be kept in the durable payment record; the current code discards it on pay success, and the comment claiming it lands via t
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-15
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
---

# Zap Protocol

## Payment Validation

The proof-of-payment (preimage) must be kept in the durable payment record; the current code discards it on pay success, and the comment claiming it lands via the wallet projection is false. (The preimage-retention fix also landed as PR #1211.)

<!-- citations: [^2e544-383] [^2e544-334] [^2e544-335] [^2e544-336] [^2e544-337] [^2e544-360] [^2e544-382] [^2e544-404] [^2e544-422] [^2e544-443] [^2e544-476] [^2e544-490] [^2e544-491] [^2e544-492] -->
## Connection Management

The WalletConnection pending diagnostic map (the heartbeat get_info probes) must be drained on response, not insert-only, to prevent unbounded in-session growth. <!-- [^2e544-338] -->

## Encryption

NWC request encryption uses NIP-04 (not NIP-44 as previously claimed in the journey); only decrypt falls back to NIP-44. <!-- [^2e544-339] -->

## Subsystem Routing

NIP-17 DM and NIP-57 zap subsystems use registered LogicalInterests exclusively (PTagRouting::Nip17DmRelays and Nip65ReadRelays respectively) with zero bespoke REQs; they are not affected. <!-- [^ab806-275] -->
