---
title: "NMP NIP-66 Crate: Relay Discovery & Monitoring"
slug: nmp-nip66
summary: "NMP's NIP-66 implementation is read-only (consume-only): it parses and surfaces NIP-66 events but never publishes kind 10166 or 30166 events."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:d4b109a1-b655-4952-9e89-9a8a1438d6a2
---

# NMP NIP-66 Crate: Relay Discovery & Monitoring

## Read-Only Implementation

NMP's NIP-66 implementation is read-only (consume-only): it parses and surfaces NIP-66 events but never publishes kind 10166 or 30166 events. [^d4b10-1]


## Crate Structure

A `nmp-nip66` crate exists (or will be created) matching the existing `nmp-nip01/22/23/29/42/51/57/77` shape, containing kind constants, validators, tag accessors, and `RelayDiscovery`/`RelayMonitor` typed views. [^d4b10-2]

## Decoupling from Relay Score

NIP-66 is decoupled from the `RelayScore` trait seam and acts only as a future provider rather than a direct routing input. Actual NIP-66 → score fusion and background subscriptions to monitor pubkeys are deferred until a concrete user-visible need arises. [^d4b10-3]

## UI and Debug Surfacing

NIP-66 data is surfaced in a 'Relay Info' UI or debug surface rather than used for autonomous routing input. [^d4b10-4]
## See Also

