---
title: Relay Transport Architecture — Synchronous Tungstenite and Channel Wiring
slug: relay-transport-architecture
summary: The relay transport uses synchronous tungstenite on a dedicated thread with recv_timeout gating, feeding events through a flume channel to the actor.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-20
updated: 2026-05-23
verified: 2026-05-20
compiled-from: conversation
sources:
  - session:c0765978-d977-4400-8274-96df7682b126
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:1670fcb8-f275-498c-975b-8bd912331ded
---

# Relay Transport Architecture — Synchronous Tungstenite and Channel Wiring

## Transport Architecture

The relay transport uses synchronous tungstenite on a dedicated thread with recv_timeout gating, feeding events through a flume channel to the actor. The relay transport lacks backpressure — if a relay fires events faster than the actor can process them, there is no mechanism to push back to the relay. The backpressure gap should be fixed with bounded channels and relay-side pause logic, not by replacing tungstenite with an async transport. Relay-list state (indexer_relays, local_write_relays) is already actor-owned; the real debt is that it uses raw shared primitives instead of typed projections. A WASM WebSocket transport must share protocol logic (backoff, keepalive FSM, frame routing) with native via a RelayTransport trait or shared relay_protocol.rs module, not duplicate implementations. A WASM Nostr client must have relay connectivity — it is a non-negotiable requirement, not an optional feature.

<!-- citations: [^c0765-6] [^c0765-7] [^c0765-8] [^1c093-24] [^1670f-18] -->
## See Also

