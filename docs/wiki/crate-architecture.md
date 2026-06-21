---
title: Crate Architecture
slug: crate-architecture
topic: crate-architecture
summary: nmp-router owns the seven-lane routing algorithm, MailboxCache implementation, selectOptimalRelays, blocked-relay post-filter, indexer-eligibility kind-gating,
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-05-27
verified: 2026-05-23
compiled-from: conversation
sources:
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# Crate Architecture

## nmp-router

nmp-router owns the seven-lane routing algorithm, MailboxCache implementation, selectOptimalRelays, blocked-relay post-filter, indexer-eligibility kind-gating, and the NIP-65 publish_relay_list ActionModule. It absorbs the kind:10002 cache and routing formerly in nmp-nip65, which is deleted. <!-- [^1670f-1] -->


Blocked relay enforcement (kind 10006) is part of the default self-kinds and must be enforced in outbox routing to prevent NMP from connecting to malicious relays. The BlockedRelayLookup trait (in nmp-core/substrate, mirroring the DmInboxRelayLookup pattern) provides the interface for reading blocked relays, with InMemoryBlockedRelayCache and Kind10006Parser implementations in nmp-router. All four build_routing_context() call sites in mailboxes.rs read blocked relays through snapshot_blocked_relays() instead of constructing an empty BlockedRelaySet. <!-- [^64f3e-4] -->

Kind numbers should live centrally in nmp-core; it is acceptable for nmp-core to know all the kind numbers anyone needs to know. The `nmp_core::kinds` module is the single canonical home for all Nostr kind constants across the workspace — protocol crates import from there rather than defining their own. nmp-router depends on nmp-core (one-way); nmp-core cannot import from nmp-router (would create a circular dependency). <!-- [^f2605-3] -->
## nmp-network

nmp-network owns sockets, reconnection, health, AUTH wire handshake, NOTICE handling, socket budget, and frame I/O. It does NOT own subscription tracking, EOSE semantics, AUTH pause policy, or EVENT deduplication. <!-- [^1670f-2] -->

## Pool Public API

The pool's public API is push-model (Sender<PoolEvent>), uses generational RelayHandle IDs, and has no send-to-all method; every send specifies a specific handle. <!-- [^1670f-3] -->

## Kernel Actor

The kernel actor is the only object that holds both router and pool handles; the router never calls pool.send directly, preserving the single-actor-thread discipline (D8) and testability. <!-- [^1670f-4] -->

## nmp-planner & nmp-store

Per-relay filter projection (partitioning authors across relays) is nmp-planner's job. Per-relay since cursors are orthogonal, would live in nmp-store, and are not in prior art (applesauce, NDK, rust-nostr). <!-- [^1670f-5] -->

## nmp-ffi & Outbox Setup

The `nmp-ffi` crate alone uses `EmptyOutboxRouter`, making `claim_profile` a no-op (zero REQs sent). The `nmp-app-template::register_defaults` must be called to install `GenericOutboxRouter` and `InMemoryMailboxCache` before profile claims can function. <!-- [^53838-4] -->

## LogicalInterest

LogicalInterest has an is_indexer_discovery: bool sentinel field (with #[serde(default)]) that replaces the former is_discovery_oneshot structural gate, allowing Tailing interests to also route through the bootstrap indexer lane. <!-- [^64f3e-5] -->

## Deleted Crates

The `nmp-nip65` crate was deleted and absorbed into `nmp-router` — references to `nmp-nip65` as a crate are stale. <!-- [^f2605-4] -->

## Known Technical Debt & Silent Failures

PendingGroupChange::drop silently clears unresolved MLS commits, which can cause group state divergence from relay. LMDB ok()?? and filter_map(res.ok()) silently swallow index-corruption errors, producing incomplete query results. The nip65_resolver module documentation claims tracing that the code never performs (false documentation). register.rs falls back to an empty Pubkey on null/invalid viewer_pubkey, causing silent anonymous mode. Lane 7 catch-all routing silently prevents routing-trace from attributing empty-outbox causes. The web chirp silently falls back to InProcessNmpClient on Worker failure without logging. <!-- [^cd2b6-2] -->
