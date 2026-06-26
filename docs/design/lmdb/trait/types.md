# LMDB Sub-Design: Store Types

The canonical type definitions live in `crates/nmp-store/src/types/` and are
re-exported where the kernel needs them. This page summarizes the active shapes
only.

## Events

`StoredEvent` wraps a verified Nostr event plus arrival metadata. `InsertOutcome`
reports insert, duplicate, replace/supersede, tombstone, rejection, and ephemeral
paths.

## Tombstones

`TombstoneRow` records deleted or expired targets so later re-inserts are
suppressed. Origins include NIP-09/kind:5 deletes, NIP-40 expiry, and internal
admin purge.

## Provenance

`ProvenanceEntry` records relay URL, first-seen time, last-seen time, and the
deterministic primary relay flag for an event.

## Coverage

`CoverageRow` records completed sync coverage for a `(filter_hash, relay)` pair:

```rust
pub struct CoverageRow {
    pub filter_hash: String,
    pub relay: String,
    pub covered_through: u64,
}
```

Rows are monotonic and downward-closed. `record_coverage` only advances the
floor.

## GC

`GcBudget` and `GcReport` describe bounded GC passes. Eviction protection is an
explicit kernel-derived pin set passed into `gc_step_with_pins`.

## Errors

`StoreError` is internal Rust state. The actor maps it to diagnostics, action
results, degraded behavior, or startup failure; no store error crosses FFI.
