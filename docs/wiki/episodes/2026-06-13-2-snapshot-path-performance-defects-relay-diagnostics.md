---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - kernel-snapshot
  - relay-diagnostics
  - flatbuffers-verifier
supersedes:
  - 2026-06-13-2-4hz-full-kernel-snapshot-cycle-is
related_claims: []
source_lines:
  - 5237-5266
  - 5286-5293
  - 5334-5360
captured_at: 2026-06-13T19:18:42Z
---

# Episode: Snapshot-path performance defects: relay-diagnostics time-string churn and FlatBuffers Verifier on trusted data

## Prior State

The 4 Hz full-kernel-snapshot cycle was assumed to be the known highest-risk performance bet (ADR-0037), with the understanding that the cost was primarily the architectural choice to re-serialize all projections every dirty tick. Relay diagnostics were always included in every snapshot by doctrine (ADR-0039). The FlatBuffers Verifier on all 29 generated typed decoders was the codegen default with no explicit rationale.

## Trigger

User reported Chirp on physical iPhone felt 'very very janky/slow'. Time Profiler trace on the device revealed relay_diagnostics verify+serialize as the single most prominent named cluster, and the FlatBuffers Verifier running on every decode of in-process trusted data as significant waste. Further investigation confirmed two specific, unjustified defects within the deliberate architectural bet.

## Decision

Identified (not yet fixed) two genuine doctrine violations/waste in the snapshot path: (1) `relay_diagnostics.rs` embeds pre-formatted relative-time strings ('3s ago', '42s ago') that change every wall-clock second, forcing the host to re-diff a 'changed' payload every tick even when nothing real happened — this violates aim.md §62 which forbids `format_ago_*` inside projection builders. (2) All 29 generated typed decoders use `getCheckedRoot` (verified path) on data produced by the same process microseconds earlier; the unchecked `getRoot` would work and is standard for trusted in-process data. No rationale was found for verification on this path.

## Consequences

- The jank diagnosis separates deliberate architectural costs (full re-serialization, always-include-all-projections) from fixable waste (time-string churn, trusted-data verification)
- Release build reduced binary from 118 MB to 29 MB and dramatically improved responsiveness, confirming debug overhead was the majority of perceived jank
- The two defects are independently fixable without changing the snapshot architecture

## Open Tail

- Fix relay_diagnostics to ship raw timestamps instead of formatted relative-time strings
- Switch Swift FlatBuffers decoders from `getCheckedRoot` to `getRoot` for trusted in-process snapshot data (or add a codegen flag)
- Consider re-profiling on the release build for honest before/after numbers

## Evidence

- transcript lines 5237-5266
- transcript lines 5286-5293
- transcript lines 5334-5360

