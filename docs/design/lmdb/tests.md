# LMDB Sub-Design: Test Plan

Store tests cover the same semantics on memory and LMDB backends where both
backends are available.

## Core Gates

| Concern | What to prove |
|---|---|
| insert path | one verified insert path updates primary rows and secondaries |
| provenance | duplicate delivery merges relay provenance without rewriting event facts |
| replaceable events | newest/equivalent deterministic winner survives restart |
| tombstones | kind:5, address tombstones, and NIP-40 expiry suppress reinsertion |
| coverage | `record_coverage` is monotonic and `get_coverage` survives restart |
| coverage floors | `coverage_max_for_filter_hash` and row iteration match backend state |
| GC | `gc_step_with_pins` preserves explicit pins and respects the budget |
| dump/export | dump output is deterministic and replayable |
