---
title: NMP Routing Lanes & Planner Architecture
slug: nmp-routing-lanes
summary: "The planner has seven routing lanes after ADR-0020 and ADR-0021: NIP-65, Hint, Provenance, UserConfigured, ClassRouted, Indexer, and AppRelay"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-26
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:41858cd2-3a5d-4ad1-bdd0-4cbe1df2dd9d
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
---

# NMP Routing Lanes & Planner Architecture

## Routing Lanes

The planner has seven routing lanes after ADR-0020 and ADR-0021: NIP-65, Hint, Provenance, UserConfigured, ClassRouted, Indexer, and AppRelay. Every relay in a plan carries a RoutingSource tag indicating why it is there, preventing the NDK mistake of collapsing all relay reasons into a single set. Indexer and AppRelay are promoted to top-level RoutingSource variants, distinct from UserConfigured, because collapsing them would repeat NDK's mistake of treating all configured relays as one giant union. The NIP-65 outbox planner routes ContactListAuthors interests through the existing Case A (NIP-65 write relays) with no new compiler work.

<!-- citations: [^41858-16] [^6e4c3-8] -->
## RoutingSource vs RelayRole

Worker-level RelayRole is a transport-lane diagnostic bucket, while planner-level RoutingSource is a planning-lane 'why this relay was chosen' tag; they align at v1 but are not identical abstraction levels. [^41858-17]
## See Also

