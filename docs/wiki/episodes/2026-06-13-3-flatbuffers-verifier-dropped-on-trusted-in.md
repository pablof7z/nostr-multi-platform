---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - flatbuffers-decode
  - nmp-core-snapshot
  - chirp-ios
supersedes: []
related_claims: []
source_lines:
  - 5275-5360
  - 5364-5367
captured_at: 2026-06-13T20:13:27Z
---

# Episode: FlatBuffers Verifier dropped on trusted in-process snapshot decode path

## Prior State

All 29 typed Swift decoders used getCheckedRoot (the verified path) on every snapshot decode. The snapshot data is produced by the same process's Rust side microseconds earlier — trusted in-process data. The Verifier ran on every frame, adding per-row validation overhead for zero security or correctness benefit.

## Trigger

Time Profiler showed FlatBuffers Verifier as a significant CPU cluster. Investigation confirmed getCheckedRoot is the codegen default with no rationale comment anywhere. User directed: 'completely remove the FlatBuffers Verifier — makes zero sense? why verify what we just created?'

## Decision

Switch snapshot/projection decode from getCheckedRoot to unchecked getRoot via the codegen template, dropping per-frame Verifier walk on trusted in-process buffers. Fix dispatched to Sonnet agent for worktree + PR.

## Consequences

- Eliminates per-frame Verifier overhead on the 4Hz snapshot path
- Codegen template change ensures it sticks (not a one-off manual edit)
- Must confirm no external/untrusted FlatBuffers sources exist that would still need verification

## Open Tail

- Broader snapshot architecture question (per-projection change gating) remains open

## Evidence

- transcript lines 5275-5360
- transcript lines 5364-5367

