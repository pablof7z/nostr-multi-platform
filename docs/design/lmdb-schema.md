# Design: Store Schema And EventStore

NMP wraps the event store behind `nmp-store::EventStore` so the kernel can use
the same semantics against the memory and LMDB backends.

## Ownership

- `nostr-lmdb` owns the canonical Nostr event rows and upstream event indexes.
- NMP-owned store code owns provenance, tombstones, coverage, hot-set/GC
  helpers, and query conveniences the kernel needs.
- The kernel actor is the writer of product state and calls the store through
  typed methods; native shells never write store rows directly.

## Current NMP-Owned Rows

| Concern | Storage | Purpose |
|---|---|---|
| provenance | `event_id -> ProvenanceRow` | records which relays delivered an event |
| coverage | `(filter_hash, relay) -> CoverageRow` | completed sync floor for read-path soundness |
| tombstones | event/address keyed rows | suppress deleted or expired events |
| secondary indexes | author/kind/tag/time/expiration keys | bounded query paths for projections and GC |

## EventStore Surface

The store exposes:

- primary and indexed event reads,
- event insert and delete-by-filter,
- provenance lookup,
- coverage read/write helpers,
- bounded GC with an explicit kernel-derived pin set,
- deterministic dump/export helpers.

All store errors stay inside Rust and are mapped to diagnostics, action
results, degraded state, or startup failure according to doctrine D6.
