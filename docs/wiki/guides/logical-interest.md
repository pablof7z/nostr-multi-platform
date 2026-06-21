---
title: LogicalInterest and the Subscription Registry
slug: logical-interest
topic: data-persistence
summary: A LogicalInterest is the actor-internal, semantics-preserving description of what a view wants the kernel to keep alive on the wire. It is the input to subscription compilation and drives both REQ subscriptions and cache-serve scans.
tags:
  - capture
volatility: warm
confidence: high
created: 2026-06-18
updated: 2026-06-18
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
---

# LogicalInterest and the Subscription Registry

## What is a LogicalInterest?

`LogicalInterest` (`crates/nmp-planner/src/interest.rs`) is the actor-internal, semantics-preserving description of what a view, action, or monitor wants the kernel to keep alive on the wire. It is the **input to subscription compilation** — NOT a Nostr filter.

```rust
pub struct LogicalInterest {
    pub id: InterestId,           // stable identity, survives recompilation
    pub scope: InterestScope,     // account-scope for mailbox resolution
    pub shape: InterestShape,     // what the consumer wants (normalised, hashable)
    pub hints: Vec<RelayHint>,    // optional routing hints
    pub lifecycle: InterestLifecycle, // OneShot vs Tailing
    pub is_indexer_discovery: bool,   // routes onto bootstrap indexer relays when true
}
```

`InterestShape` carries authors, kinds, tag filters, address coordinates, since/until, and limit. It is deterministically hashable for dedup.

## Role in the System

A `LogicalInterest` drives two parallel mechanisms:

1. **REQ subscriptions (live wire):** The planner (`nmp-planner`) compiles the active interest set into a `CompiledPlan` that maps relay connections to Nostr `REQ` filters. Recompilation fires on follow-list changes, relay reconnects, or account switch.

2. **Cache-serve scans (store-first):** `shape_to_store_queries` (`crates/nmp-core/src/kernel/cache_serve/queries.rs`) translates the `InterestShape` into `StoreQuery` variants (one per covered LMDB index). The scan feeds matching store events through the projection path before the relay delivers them.

## InterestRegistry

`InterestRegistry` (`crates/nmp-core/src/subs/registry.rs`) maintains the active interest set, keyed by `(SubScope, SubKey)`. Multiple view owners can share the same interest; the registry tracks owner refcounts via `drop_owner`. `iter_active()` returns the live set for planner recompilation and cache-serve targeting.

Note: the registry is a *forward* index (interest → owners). It is NOT a reverse index from event attributes → interested views. The `matches_active_open_interest` path in timeline routing does an O(active-interests) walk per inbound event (a known limitation; the ADR-0001 composite reverse index is not yet implemented).

## Lifecycle

- `OneShot`: closed after EOSE from the relay.
- `Tailing`: kept open indefinitely (e.g., follow feed, DM inbox).

`is_indexer_discovery = true` routes the interest onto bootstrap indexer relays when the author's NIP-65 mailbox is unknown and no app relays are configured (PD-033-C outbox extension).

## Cache-Serve Wakeups

`cache_serve_wakeups` is a `BTreeSet<u64>` coalescing buffer on Kernel. Wakeups fire only from the `accepted.rs` live-ingest path (never from `feed_served_event`), and drain rides the existing actor idle tick (no new timers, D8-compliant). <!-- [^129d2-104] -->
