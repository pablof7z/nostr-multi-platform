---
title: Showcase Relay Reachability — Data Lives on nos.lol, Not Default Seeds
slug: showcase-relay-data-reachability
summary: Showcase events used by nmp-gallery are absent from default seeded relays but reachable via nos.lol; the fix uses nevent relay hints rather than changing seed relays.
tags:
  - nmp-gallery
  - showcase
  - relays
  - nos.lol
  - outbox
  - nip-65
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-25
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:c8c2902c-43a6-4b1c-8215-1732dc266895
---

# Showcase Relay Reachability — Data Lives on nos.lol, Not Default Seeds

> Showcase events used by nmp-gallery are absent from default seeded relays but reachable via nos.lol; the fix uses nevent relay hints rather than changing seed relays.

Relay Data Availability

The showcase events used in nmp-gallery are not available on the two relays seeded by default from showcase-references.json (purplepag.es and relay.primal.net). They are available on nos.lol. Verified via direct websocat queries to each relay. [^6a951-101]

Smoke Test Output Lie

The nmp-gallery-tui --smoke binary prints a hardcoded message 'seeded relays are purplepag.es, nos.lol, relay.damus.io, relay.nostr.band' (main.rs:345) that does not reflect its actual relay configuration. The actual seeds come from showcase::references().relays which returns only purplepag.es + relay.primal.net. This stale hardcoded string was misleading during the embed-loading investigation. [^6a951-102]

Event Availability by Relay

Verified with direct websocat REQ queries: pablof7z's kind:1 note (276d69d6…) is absent on purplepag.es and relay.primal.net, present on nos.lol. Gigi's kind:30023 article is absent on purplepag.es and relay.primal.net, present on nos.lol. pablof7z's kind:9802 highlight is absent on relay.primal.net, present on nos.lol. pablof7z's kind:10002 (NIP-65 outbox) is present on purplepag.es listing write relays 140.f7z.io, pyramid.fiatjaf.com, and r.f7z.io. [^6a951-103]

NIP-65 Outbox Chain Viability

The full NIP-65 outbox chain is viable end-to-end for showcase data. pablof7z's kind:10002 is on purplepag.es (the seeded indexer), listing write relays 140.f7z.io and pyramid.fiatjaf.com — both of which have the note. Gigi's kind:10002 is also on purplepag.es, listing write relays including relay.dergigi.com and nostr.wine. The data is reachable via outbox; the issue is that cold-start nevent claims do not perform Phase 2 outbox expansion. [^6a951-104]

Design Decision: Showcase Relay Strategy

The user chose 'outbox only' — no hardcoded content relay should be added to showcase-references.json. The kernel's NIP-65 outbox model should reach the events. An earlier session attempted to add nos.lol to showcase-references.json and the user reverted it. The correct fix uses nevents with relay hints pointing to nos.lol (which has the events), relying on the fact that nevent claims follow relay hints first by design. [^6a951-105]


## Design Decision: Showcase Relay Strategy

The user chose 'outbox only' — no hardcoded content relay should be added to showcase-references.json. The kernel's NIP-65 outbox model should reach the events. An earlier session attempted to add nos.lol to showcase-references.json and the user reverted it. The correct fix uses nevents and naddrs with relay hints pointing to nos.lol (which actually serves the events), not stale hints pointing to relays that lack them. (Previously: The correct fix uses nevents with relay hints pointing to nos.lol, which has the events, relying on the fact that nevent claims follow relay hints first by design.) [^6a951-116]

## Relay Data Availability

The NmpGallery app seeds 3 bootstrap relays (purplepag.es, relay.damus.io, nos.lol) at startup so kind:0 fetches have a routing target even with no logged-in user. The nmp-app-gallery JNI bootstrap connects to relays wss://purplepag.es, wss://relay.damus.io, and wss://nos.lol.

<!-- citations: [^53838-18] [^c8c29-5] -->
## See Also
- [[nevent-cold-start-outbox-expansion-gap|NIP-65 Outbox Expansion Gap for Cold-Start nevent Claims]] — related guide
- [[nmp-gallery-verification-matrix|NMP Gallery Verification Matrix — 64-Cell Cross-Platform Quality Gate]] — related guide

