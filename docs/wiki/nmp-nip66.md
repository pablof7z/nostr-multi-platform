---
title: NIP-66 — Relay Discovery and Monitoring (nmp-nip66 Crate)
slug: nmp-nip66
summary: NMP consumes NIP-66 events read-only and does not publish kind 10166 or 30166 events
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

# NIP-66 — Relay Discovery and Monitoring (nmp-nip66 Crate)

## Overview

NMP consumes NIP-66 events read-only and does not publish kind 10166 or 30166 events. NIP-66 support is delivered as a standalone `nmp-nip66` crate containing kind constants, validators, tag accessors, and `RelayDiscovery`/`RelayMonitor` typed views. [^d4b10-5]


## Kind Constants

The `RelayDiscovery` kind constant is 30166, addressable on `d=<relay url>`. The `RelayMonitor` kind constant is 10166. [^d4b10-6]
## See Also

