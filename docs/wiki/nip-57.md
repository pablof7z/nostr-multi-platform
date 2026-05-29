---
title: NIP-57
slug: nip-57
summary: The nostr protocol for Lightning zaps, implemented in the `nmp-nip57` crate as part of the nostr-multi-platform project.
tags:
  - zaps
  - lightning
  - nip57
volatility: warm
confidence: low
created: 2026-05-29
updated: 2026-05-29
verified: 
compiled-from: codebase
sources:
  - codebase
---

## Overview

NIP-57 specifies a protocol for sending and receiving Lightning zaps on nostr. In the nostr-multi-platform codebase, the client-side implementation lives primarily in the `nmp-nip57` crate, with supporting abstractions and kernel integration provided by `nmp-core`.

## Protocol Elements

- Two new event kinds are defined: `9734` (zap request) and `9735` (zap receipt)  \(crates/nmp-nip57/src/kinds.rs:1\).
- Zaps are triggered by `lud16` (lightning address) or `lud06` (LNURL) fields in the recipient's profile  \(crates/nmp-core/src/kernel/nostr.rs:42,47,74\).
- The client constructs a signed `kind:9734` event containing the amount and an optional comment  \(crates/nmp-nip57/src/action.rs:16,445\).
- The request is delivered to the LN provider's callback URL with an `amount` query parameter  \(crates/nmp-nip57/src/lnurl/mod.rs:12,467\).
- The resulting `kind:9735` receipt is published by the LN provider to the relays indicated in the request's `relays` tag  \(crates/nmp-core/src/substrate/protocol.rs:161,164; crates/nmp-nip57/src/lnurl/mod.rs:33,309\).
- LN providers must advertise `allowsNostr` support  \(crates/nmp-nip57/src/lnurl/mod.rs:462\).

## Implementation Architecture

### `nmp-core` support

- The `RecipientRelayLookup` trait resolves the recipient's NIP-65 write relays for inclusion in the `relays` tag  \(crates/nmp-core/src/substrate/protocol.rs:34,182\).
- The LNURL fetch is dispatched as a `ProtocolCommand` and runs on a background worker thread  \(crates/nmp-core/src/actor/mod.rs:642; crates/nmp-core/src/substrate/host_op_handler.rs:71\).
- The kernel ingests `kind:9735` events for zap aggregation and projection  \(crates/nmp-core/src/kernel/ingest/event.rs:126; crates/nmp-core/src/kernel/ingest/mod.rs:454\).

### `nmp-nip57` crate

- Provides the `ActionModule` for initiating a zap  \(crates/nmp-nip57/src/action.rs:1\).
- Contains an LNURL-pay fetcher that respects the NIP-57 callback flow  \(crates/nmp-nip57/src/lnurl/mod.rs:1\).
- Manages a subscription for self-zap receipts so the user can see incoming zaps  \(crates/nmp-nip57/src/interests.rs:1,7\).
- Handles receipt parsing and validation, including extraction of the `lnurl` tag  \(crates/nmp-nip57/src/lnurl/mod.rs:185,189\) and recipient lookups  \(crates/nmp-nip57/src/lnurl/mod.rs:315\).

NIP-57 thus enables a standardised, interoperable way to send and receive Lightning payments on nostr, fully integrated into the nostr-multi-platform actor and protocol infrastructure.
