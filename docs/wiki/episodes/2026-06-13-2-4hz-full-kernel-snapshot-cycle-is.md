---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-core-snapshot
  - kernel-emit-cycle
  - flatbuffers-performance
supersedes: []
related_claims: []
source_lines:
  - 5194-5280
  - 5286-5307
captured_at: 2026-06-13T18:51:06Z
---

# Episode: 4Hz full-kernel-snapshot cycle is the performance bottleneck on physical device

## Prior State

The 4Hz full-kernel-snapshot cycle's runtime cost was unquantified; app perceived as janky on physical iPhone was attributed to Debug build but not profiled

## Trigger

Time Profiler trace on physical iPhone 17 Pro Max (3,637 samples over 20s) showed: FlatBuffers encode/decode/verify ~9% CPU, Rust snapshot serialize ~8%, debug-only safety checks 6%, crypto/signature verify ~4%. Specific wastes identified: RelayDiagnosticsRow re-encoded every tick regardless of visible screen; Swift-side FlatBuffers Verifier runs on every decode of trusted in-process Rust data

## Decision

The 4Hz full-kernel-snapshot cycle is the dominant architectural cost; two specific wastes identified that persist even in Release builds: (1) all projections including relay-diagnostics are serialized every tick unconditionally, and (2) the FlatBuffers Verifier validates every row on the Swift side even though the data originates from in-process trusted Rust

## Consequences

- Debug build is not representative of shipping performance — release build needed for real assessment
- RelayDiagnosticsRow serialization should be conditional on the diagnostics screen being visible
- Trusted-path Verifier skip on in-process Rust→Swift FlatBuffers decode is an optimization opportunity
- A Release build was initiated for the physical iPhone to get representative performance numbers

## Open Tail

- Root-cause investigation of why all projections are serialized every tick is in progress — user explicitly asked WHY this design exists and assistant is researching the code
- Release build profiling needed to quantify the real (non-debug) cost of the snapshot cycle
- No decision yet on whether to adopt selective/diffed snapshot emission or trusted-path Verifier skipping

## Evidence

- transcript lines 5194-5280
- transcript lines 5286-5307

