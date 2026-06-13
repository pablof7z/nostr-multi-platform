---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: reversal
status: superseded
subjects:
  - kernel-snapshot
  - projection-emission
  - adr-0037
  - adr-0053
supersedes: []
related_claims: []
source_lines:
  - 5275-5280
  - 5393-5425
  - 5456-5467
  - 5618-5626
captured_at: 2026-06-13T20:33:24Z
---

# Episode: Full-snapshot-per-tick model superseded by incremental projection emission (ADR-0053)

## Prior State

ADR-0037 designates the 4Hz full-snapshot-every-tick model as 'the single highest-risk performance bet in the architecture.' aim.md §10 states: 'State crosses FFI as a full Cloned snapshot by default; granular updates are an optimization, not a default.' The kernel re-serializes every projection unconditionally on every dirty tick; the host replaces its entire state; SwiftUI re-diffs the whole decoded tree. Per-projection content-hash gating existed only for generic-JSON projections (snapshot_registry.rs), not for the typed sidecars on the hot path.

## Trigger

User found the architecture 'completely stupid and unacceptable' after Time Profiler showed full re-serialization cost on physical device. Analysis confirmed the 'simplicity vs fragile deltas' binary is false: per-projection change-gating (O(changed-projections) emission) preserves the self-healing snapshot/rev correctness invariant while eliminating waste, and is how every sane state-sync system works. The full re-serialize + full decode cost is O(total-state) per tick when it should be O(changed-projections).

## Decision

Supersede with ADR-0053 (PR #1331): incremental projection emission. O(changed-projections) as the minimum acceptable floor. Per-projection revision/content-hash gating extended from snapshot_registry to the typed sidecar hot path. Preserves the self-healing snapshot/rev correctness invariant. Wire contract: baseline + per-key {rev, changed|unchanged|cleared, payload?} where omitted = unchanged and cleared is explicit.

## Consequences

- Host apply must become rev-aware (avoid reassigning @Published slots for unchanged projections)
- Projection closures currently run on the actor thread — heavy encode competes with relay ingest (may need to move off-thread)
- Existing quick fixes (verifier removal, raw timestamps) compose with the redesign
- Per-projection gating mechanism already exists for generic-JSON projections — extending to typed sidecars is largely extending an existing pattern

## Open Tail

- Intra-projection deltas (within a single projection) are on the table but not committed to — left to the Opus architect's research
- Implementation timeline and migration path not yet finalized

## Evidence

- transcript lines 5275-5280
- transcript lines 5393-5425
- transcript lines 5456-5467
- transcript lines 5618-5626

