---
title: Live-Relay Tooling for v1 Verification Harness
slug: live-relay-tooling-verification-harness
summary: "User-provided live-relay tooling for v1 exit-criteria verification: nak serve (in-memory) and relay.primal.net."
tags:
  - v1
  - testing
  - relay
  - verification
  - nak
  - F-02
  - F-04
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-28
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:d366b3c7-f7a7-49d5-9961-625037c7deb6
---

# Live-Relay Tooling for v1 Verification Harness

> User-provided live-relay tooling for v1 exit-criteria verification: nak serve (in-memory) and relay.primal.net.

## Overview

The F-02 (DM cold-start receive) and F-04 (Zap E2E) v1 exit-criteria verification tasks require a live-relay / live-NWC validation harness. The user provided two concrete options: nak serve (in-memory relay) for local testing, and relay.primal.net as a public relay endpoint. [^4edd4-168]

## nak serve — In-Memory Relay

nak serve provides an in-memory Nostr relay suitable for local testing. This can serve as the controlled relay environment for F-02 DM cold-start verification and F-04 Zap E2E round-trip tests. [^4edd4-169]

## relay.primal.net

relay.primal.net is a public relay that can serve as the live endpoint for verification harnesses. The endpoint is available at `wss://relay.primal.net`. nmp-desktop connects to `wss://relay.primal.net` to stream live notes. The tooling note was saved to memory for when the F-02/F-04 verification harness work begins.

<!-- citations: [^4edd4-170] [^4edd4-222] [^d366b-4] -->
## See Also

