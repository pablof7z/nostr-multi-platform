---
title: "Chirp DMs: NIP-17 Gift-Wrap and Relay Routing"
slug: chirp-dms
topic: app-dms
summary: "iOS DMs are sent via NIP-17 gift-wrap (kind:1059) over wss-only relay targets"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp DMs: NIP-17 Gift-Wrap and Relay Routing

## DM Transport

iOS DMs are sent via NIP-17 gift-wrap (kind:1059) over wss-only relay targets. Plaintext `ws://` relay URLs are rejected. DM send and receive work end-to-end, confirmed by a two-device real-relay round trip: a message sent from one simulator is received and decrypted on a second independent simulator, with the sender's profile name resolved by the recipient.

<!-- citations: [^dcc80-f11fc] [^dcc80-77ab8] -->
