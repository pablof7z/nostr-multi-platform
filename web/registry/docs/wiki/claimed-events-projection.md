---
title: Refs Event Projection
slug: claimed-events-projection
summary: "The `refs.event` row-delta projection keys naddr (addressable) event refs by `kind:pubkey:d_tag` coordinate and nevent/note refs by hex64 event id."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-06-27
verified: 2026-06-27
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Refs Event Projection

## Key Structure

The current event-reference source is `refs.event`, a row-delta projection keyed by `primary_id`.

For naddr/address refs, `primary_id` is the canonical `kind:pubkey:d_tag` coordinate. For nevent/note refs, `primary_id` is the hex64 event id.

Each row payload is a single-entry KCEV event row. Hosts merge `refs.event` through `RefEventStore`; render-facing envelope maps are derived from that store, not from the old whole-map `claimed_events` projection.

## Relay Resolution for Nevent Claims

For nevent (event-id) refs, the kernel follows the relay hint first; outbox expansion is not reliable because nevents don't guarantee an embedded pubkey. [^6a951-4]

## Relay Hint Re-encoding

The showcase nevents were re-encoded with `wss://nos.lol` as the relay hint (replacing `relay.primal.net` which lacks the events), and the article naddr was re-encoded with `wss://nos.lol` (replacing the undialable `https://dergigi.com` HTTP blog URL). [^6a951-5]
## See Also
