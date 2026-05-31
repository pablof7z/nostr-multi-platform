---
title: NIP-46 Nostr Connect Implementation
slug: nip46-nostr-connect-implementation
summary: "nmp_app_nostrconnect_uri() hardcodes wss://relay.primal.net as the default NIP-46 QR-code relay."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-19
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:cc7dc68a-1fcd-49fe-98be-198f17b6d59e
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
---

# NIP-46 Nostr Connect Implementation

## Default Relay

nmp_app_nostrconnect_uri() hardcodes wss://relay.primal.net as the default NIP-46 QR-code relay. (Previously: wss://relay.damus.io.)

<!-- citations: [^cc7dc-6] [^fd809-4] -->
## See Also

