---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: reversal
status: active
subjects:
  - projection-emission
  - adr-0053
  - snapshot-transport
supersedes:
  - 2026-06-13-3-adr-0053-invert-projection-emission-from
related_claims: []
source_lines:
  - 5973-6111
captured_at: 2026-06-13T21:45:23Z
---

# Episode: Projection emission default inverted: incremental-by-default, snapshot-as-resync

## Prior State

aim.md §10 Doctrine #12: 'Snapshots by default, granular updates as optimization' — the kernel emits the full projection set every tick; incremental is a someday-optimization. ChangeGate only memoizes closures (values still serialized/emitted/decoded every tick); the hot path (typed sidecars + built-ins) is completely ungated.

## Trigger

Owner directive for zero-debt, performant architecture; ADR-0053 research showed the gated part is cold and the hot part is ungated — an inversion. Baseline stress measurement confirmed: 100k-event flood at 6.4 Hz, ~4 KB max payload/emit, net heap 22 B/emit, but unchanged projections are re-serialized every tick (waste not yet measured).

## Decision

Flip the default: per-projection revision-gated incremental emission is the steady-state; full snapshot is the resync/cold-start fallback. Wire contract: {key, projection_rev, state, payload?} — Changed→decode+apply; Unchanged (payload omitted)→host reuses prior buffer; Cleared→explicit drop. Absence==Unchanged, never Cleared. Self-healing via per-key monotonic rev (last-rev-wins, no op-log/CRDT), session_id + snapshot_epoch for reset/rebaseline. Forks resolved: (1) global snapshot_epoch, (2) omission==Unchanged + explicit Cleared (2-valued), (3) reuse existing init-time wall-clock stamp, (4) row-deltas NOT deferred — measured empirically then decided. 7-rung implementation ladder: 0-instrument → 1-kernel rev manifest → 2-wire fields (byte-additive) → 3-omit-unchanged + host reuse (the floor) → 4-resync FFI + epoch on resets → 5-compose with host-declared projections → 6-feed row-deltas (separate ADR, only after measurement).

## Consequences

- Full-snapshot-on-every-tick becomes historical/resync-only; all transport code must migrate
- ChangeGate's closure-memoization becomes insufficient — transport-level gating replaces it for hot-path built-ins
- Row-deltas for feed viewport explicitly deferred to measurement, not indefinitely punted — stress data decides
- ADR numbering must be renumbered off the 0053 collision (two existing 0053s on master; next free is 0054)
- PR #1332 (relay-diagnostics raw timestamps) is a hard prerequisite — relative-time strings would poison the rev gate

## Open Tail

- Rung-0 instrumentation must measure per-tick re-encode waste (unchanged projections re-serialized) against the S3 baseline captured in this session
- Row-delta decision for nmp-feed viewport deferred to empirical measurement post-Rung-3

## Evidence

- transcript lines 5973-6111

