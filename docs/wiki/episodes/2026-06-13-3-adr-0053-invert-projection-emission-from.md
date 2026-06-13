---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: reversal
status: superseded
subjects:
  - projection-emission-architecture
  - adr-0053
  - aim-md-10
  - snapshot-transport
supersedes:
  - 2026-06-13-4-full-snapshot-per-tick-model-superseded
related_claims: []
source_lines:
  - 5336-5341
  - 5393-5425
  - 5456-5468
  - 5508-5525
  - 5971-6007
captured_at: 2026-06-13T21:09:24Z
---

# Episode: ADR-0053: Invert projection emission from full-snapshot-default to incremental-default

## Prior State

aim.md §10 and Doctrine #12: "State crosses FFI as a full Cloned snapshot by default; granular updates are an optimization, not a default." ADR-0037 called the 4 Hz snapshot "the single highest-risk performance bet in the architecture." Every dirty tick re-serializes all ~15 projections unconditionally — O(total state) per tick, not O(changed state). The typed-FlatBuffers sidecar (ADR-0037) made each re-encode cheaper but did not make it incremental.

## Trigger

Performance investigation + architectural analysis revealed: (1) the full-snapshot cost scales with total state, not what changed — 5,000 timeline events re-serialized for one relay RTT tick; (2) the ADR justified this with a false binary (full snapshots vs fragile hand-written deltas), ignoring the correct middle: per-projection content-hash/revision gating; (3) the diff was not eliminated but relocated to SwiftUI's AttributeGraph, which re-diffs the whole decoded tree every frame anyway — making the full re-serialize a pure tax on top of an unavoidable host diff. Codex second opinion confirmed and added that per-projection change-gating already exists for generic-JSON projections (snapshot_registry.rs) but is missing from the typed-sidecar hot path.

## Decision

ADR-0053 inverts the default: the transport becomes incremental by default (per-projection revision-gated), with full snapshot as the resync/cold-start fallback. aim.md §10 and Doctrine #12 are superseded. The PR ladder (Rungs 0–6) starts with instrumentation, adds kernel-side revision tracking, extends the wire contract with projection_rev/state/snapshot_epoch, then produces omit-unchanged-keys semantics, host-requested resync, composition with host-declared projections, and defers intra-projection row deltas to a separate future ADR. The correctness invariant (host state = pure function of latest frame + monotonic rev) is preserved.

## Consequences

- Emission cost drops from O(total state) per dirty tick to O(changed projections) per dirty tick
- The existing snapshot_registry change-gate mechanism for generic-JSON projections must be extended to the typed-sidecar hot path
- Host must become rev-aware to avoid reassigning @Published slots for unchanged projections (otherwise SwiftUI invalidation still fires broadly)
- Projection closures running on the actor thread must be offloaded to avoid competing with relay ingest
- Full-snapshot emission is not deleted — it becomes the resync/cold-start/recovery path
- aim.md §10 and Doctrine #12 must be updated to reflect the new default
- Three open design forks in ADR-0053 await owner sign-off

## Open Tail

- Owner sign-off on ADR-0053 and its three open design forks
- Rung 0 (instrumentation) is the first implementation step — measure before optimizing
- Intra-projection row deltas deferred to a separate ADR (Rung 6)
- Must compose with Decision-2 redesign (Rung 5)

## Evidence

- transcript lines 5336-5341
- transcript lines 5393-5425
- transcript lines 5456-5468
- transcript lines 5508-5525
- transcript lines 5971-6007

