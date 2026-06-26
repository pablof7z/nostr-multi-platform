# LMDB Sub-Design: Coverage And Provenance

The current read-path floor authority is the K3 coverage ledger.

## Coverage Ledger

Sub-db: `nmp-coverage`.

Key: `filter_hash || relay || covered_through`, encoded by the store coverage
helpers so mem and LMDB backends share semantics.

Value: `CoverageRow { filter_hash, relay, covered_through }`.

`covered_through` is downward-closed: a row asserts that the store has
completed sync coverage for `[0, covered_through]` for that exact
`(filter_hash, relay)` pair. Floors are read from completed coverage, not from
the newest stored event that happens to match a shape.

The public store methods are:

- `record_coverage(filter_hash, relay, covered_through)`
- `get_coverage(filter_hash, relay)`
- `coverage_max_for_filter_hash(filter_hash)`
- `coverage_rows_for_filter_hash(filter_hash)`

EOSE and NEG-DONE are the write sources. Floored REQs must not over-claim
coverage below their own floor.

## Provenance

Sub-db: `provenance`.

Key: `event_id`.

Value: `ProvenanceRow { sources: Vec<ProvenanceEntry> }`.

Each `ProvenanceEntry` records the relay URL, first-seen timestamp,
last-seen timestamp, and deterministic primary flag. Duplicate event delivery
updates provenance without rewriting the primary event row.

Provenance feeds diagnostics and routing decisions; it is not a substitute for
coverage. Provenance says where a row was seen. Coverage says which time window
has completed sync.

## Migration Note

There is no active per-domain LMDB migration registry. App-specific durable
domain state belongs in the owning app crate or a reusable protocol/substrate
crate, then surfaces through projections/actions.
