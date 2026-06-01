---
title: Claimed Events Projection
slug: claimed-events-projection
summary: "The `claimed_events` projection keys naddr (addressable) claims by `kind:pubkey:d_tag` coordinate and nevent (event-id) claims by hex64 event id."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Claimed Events Projection

## Key Structure

The `claimed_events` projection keys naddr (addressable) claims by `kind:pubkey:d_tag` coordinate and nevent (event-id) claims by hex64 event id. [^6a951-3]


## Relay Resolution for Nevent Claims

For nevent (event-id) claims, the kernel follows the relay hint first; outbox expansion is not reliable because nevents don't guarantee an embedded pubkey. [^6a951-4]

## Relay Hint Re-encoding

The showcase nevents were re-encoded with `wss://nos.lol` as the relay hint (replacing `relay.primal.net` which lacks the events), and the article naddr was re-encoded with `wss://nos.lol` (replacing the undialable `https://dergigi.com` HTTP blog URL). [^6a951-5]
## See Also

