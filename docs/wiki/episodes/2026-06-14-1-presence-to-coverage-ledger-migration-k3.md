---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - coverage-ledger
  - since-floor
  - watermark
  - neg-open
  - eviction-coherence
supersedes:
  - 2026-06-14-3-presence-floor-coverage-ledger-migration-k3
  - 2026-06-14-4-coverage-ledger-replaces-presence-as-since
related_claims: []
source_lines:
  - 5967-6404
captured_at: 2026-06-14T15:39:42Z
---

# Episode: Presence-to-coverage ledger migration (K3)

## Prior State

The since-floor was computed from presence (newest stored event matching a shape). Presence ≠ coverage: a subscription whose EOSE completed was floored at the newest stored event, suppressing re-fetch of cached events but also preventing repair of below-floor gaps. This created a class of permanent backfill holes, especially for follow feeds with ≥50 author×kind fanout. Three hand-synced copies of the shape→query mapping existed, one of which had already drifted to an unsafe policy.

## Trigger

Read-path review finding H2: presence-is-not-coverage. NEG-OPEN inherited the floored since, so set reconciliation only covered [floor, ∞) — exactly the window declared boring — and could not repair below-floor gaps. Production LRU eviction (HOT_EVENT_CEILING=10k) could delete below an active covered_through, leaving the ledger over-claiming.

## Decision

Migrate from presence-based floor to per-(filter, relay) coverage ledger, staged in 6 steps behind a default-off flag: (A) NEG-OPEN un-flooring so NEG-eligible shapes self-heal over the full window; (B) three heuristic soundness patches (address-pointer min/abort, NEG-OPEN liveness deadline, truncated-serve floor refusal); (C) collapse all floor→serve mappings to one shape_to_store_queries (found and deleted a drifted residual copy using the pre-B1 unsafe max policy); (D1) additive ledger write path at EOSE/NEG-DONE behind coverage_ledger_enabled; (D2) read-swap surgery — with flag ON and no coverage row, REFUSE the floor (full window) rather than fall back to presence; (D3) eviction⇄ledger coherence via atomic CoverageGuard on both backends. Flag remains default-off; production byte-identical to pre-K3.

## Consequences

- Journey test proves H1 backfill-suppression bug fixed flag-on, still-broken flag-off
- All six stages landed on master (#1372–#1416) with flag default-off
- Single shape_to_store_queries mapping now governs both cache-serve and floor (Stage C drift bug eliminated)
- Eviction atomically lowers coverage rows on both Mem and LMDB backends
- The flag default-on flip and Stage E (delete presence heuristic) are deferred as owner-gated release decisions — not autonomously flipped
- No FFI surface added; kernel/mod.rs kept under 2797-line baseline via build_watermark_fn extraction

## Open Tail

- Stage E: delete the presence heuristic and the flag-off fallback, leaving coverage ledger as sole floor source
- Release-cut: flip coverage_ledger_enabled default-on — external git-rev-pinning consumers must pin across this behavioral change
- Correct the false [LANDED M4] docs

## Evidence

- transcript lines 5967-6404

