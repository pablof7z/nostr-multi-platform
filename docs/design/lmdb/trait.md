# LMDB Sub-Design: EventStore Trait

`EventStore` is the store abstraction used by the kernel. The concrete
implementations live in `nmp-store`; both memory and LMDB backends implement the
same semantics.

## Reads

The store supports primary lookup, indexed scans, generic `StoreQuery`
visitation, tombstone lookup, provenance lookup, and coverage lookup.

Scans are bounded and newest-first where the caller needs feed-like behavior.
Visitor APIs are preferred for hot paths so the caller does not have to
materialize large intermediate vectors.

## Writes

`insert` is the single event write path. It verifies and stores events, applies
replaceable/delete/expiration invariants, updates secondaries, and records
provenance.

`delete_by_filter` is an internal admin/GC path, not a remote filter passthrough.

## Coverage

Coverage helpers are the current sync-floor API:

```rust
fn record_coverage(&self, filter_hash: &str, relay: &str, covered_through: u64);
fn get_coverage(&self, filter_hash: &str, relay: &str) -> Option<CoverageRow>;
fn coverage_max_for_filter_hash(&self, filter_hash: &str) -> Option<u64>;
fn coverage_rows_for_filter_hash(&self, filter_hash: &str) -> Vec<(String, u64)>;
```

Coverage rows are downward-closed. They are written by completed sync paths and
read by the kernel floor logic.

## GC

GC receives an explicit pin set from the kernel:

```rust
fn gc_step_with_pins(
    &self,
    budget: GcBudget,
    now_secs: u64,
    pins: &HashSet<EventId>,
) -> Result<GcReport, StoreError>;
```

The removed persisted claim-register API is not part of the current trait.

## Errors

Store methods return Rust errors internally. The actor maps them to diagnostics,
degraded state, action failures, or startup failure. Store errors do not cross
FFI.
