# ADR-0056 — K3 coverage ledger

- **Status:** Accepted / implemented
- **Date:** 2026-06-14
- **Keystone:** K3 read-path soundness
- **Doctrine:** D2 negentropy-first, D8 bounded work
- **Relates to:** ADR-0045, ADR-0057

## Context

A stored event proves presence, not relay coverage. A read subscription must not
infer that a relay has been fully synced for a filter just because the local store
contains a newer matching event. Doing that can suppress older events that were
never fetched.

The since floor must therefore be derived from completed sync coverage, not from
stored event presence.

## Decision

NMP uses a coverage ledger as the sole since-floor authority for read
subscriptions.

The ledger records coverage per `(filter_hash, relay)`:

```rust
pub struct CoverageRow {
    pub filter_hash: u64,
    pub relay: String,
    pub covered_through: u64,
}
```

Coverage is written only when a sync completion proves a downward-closed range:

- an un-floored REQ records coverage on EOSE;
- a NIP-77 reconciliation records coverage on NEG-DONE;
- a floored REQ does not claim coverage below its floor.

When compiling a relay filter, the kernel reads the coverage row for the
canonical filter hash and target relay. If a row exists, `since` can be floored
to `covered_through + 1`. If no row exists, the request remains un-floored and
the relay can return the full range.

## Negentropy

NIP-77 reconciliation uses an un-floored local item set so it can repair gaps
below any plain-REQ floor. Statically tiny filters can stay on plain REQ; broad
history and tag filters use NIP-77 when the relay and local store can reconcile
the exact result set.

## Cache-Serve And Pinning

Store-to-projection replay remains the single cache-serve mechanism. The coverage
ledger only decides what the next relay request may omit.

Finite durable-retention GC must keep coverage and storage coherent. If an
explicit durable quota deletes an event at or below a covered range, the same
store operation lowers or clears the affected coverage row so the next compile
will re-fetch the hole.

## Tests

The coverage design is guarded by:

- journey coverage proving that following an author after seeing a stray thread
  reply backfills that author's full history;
- store tests proving coverage lowering on Mem and LMDB backends;
- kernel GC tests proving pinning and coverage guards use the same floor source.

## Consequences

- Read-path floors reflect completed relay syncs, not local presence.
- Below-floor gaps can self-heal through un-floored reconciliation.
- Cache-serve, request compilation, and finite-retention GC share one coverage
  authority.
