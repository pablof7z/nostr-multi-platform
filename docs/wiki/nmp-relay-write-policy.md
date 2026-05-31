---
title: NMP Relay Write Policy & Allowed Relays
slug: nmp-relay-write-policy
summary: r.f7z.io does not allow writing by anyone; relay.primal.net is the correct public relay for writes.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-28
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:d366b3c7-f7a7-49d5-9961-625037c7deb6
---

# NMP Relay Write Policy & Allowed Relays

## Write Policy

r.f7z.io does not allow writing by anyone; relay.primal.net is the correct public relay for writes. nmp-desktop connects to wss://relay.primal.net to stream live notes. Kind:10006 (blocked relay list) is enforced in outbox routing to prevent WebSocket connections to blocked (malicious) relays. BlockedRelayLookup substrate trait and InMemoryBlockedRelaysCache populate BlockedRelaySet in all four build_routing_context() call sites in mailboxes.rs instead of creating empty sets.

<!-- citations: [^fe79b-12] [^64f3e-5] [^d366b-4] -->
## See Also

