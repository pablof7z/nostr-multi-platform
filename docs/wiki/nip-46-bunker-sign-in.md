---
title: NIP-46 Bunker Sign-In Capability
slug: nip-46-bunker-sign-in
summary: NIP-46 bunker is a first-class v1 sign-in capability, making V-14 (BunkerConnectionState projection) and V-78 in-scope for v1
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-03
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
---

# NIP-46 Bunker Sign-In Capability

## NIP-46 Bunker Sign-In

NIP-46 bunker is a first-class v1 sign-in capability, making V-14 (BunkerConnectionState projection) and V-78 in-scope for v1. nak bunker is the designated live remote signer for testing off-actor NIP-46 paths. NIP-46 bunker sign-and-return resolves asynchronously via the SignerOpResolved re-entry pattern (same as the LNURL re-entry in nmp-nip57), surfacing results in the signed_events projection keyed by correlation_id. V-78 (bunker zaps) is fixed by threading sign_active_nonblocking into the ProtocolCommand path instead of requiring active_local_keys().

<!-- citations: [^4edd4-14] [^f1b74-29] [^f1b74-37] -->
