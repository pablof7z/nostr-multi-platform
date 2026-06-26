---
type: episode-card
date: 2026-06-26
session: 1077a92b-e2b0-457d-870e-5e12e4f524cf
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1077a92b-e2b0-457d-870e-5e12e4f524cf.jsonl
salience: root-cause
status: active
subjects:
  - snapshot
  - projection
  - browser-runtime
  - d4-contract
  - d6-contract
  - nmp-browser-runtime
supersedes: []
related_claims: []
source_lines:
  - 3399-3410
  - 3415-3415
  - 3416-3433
  - 3437-3488
captured_at: 2026-06-26T08:04:10Z
---

# Episode: Snapshot/projection contract: root-cause diagnosis and fixes for transactional-merge, D6-poison, provider-readiness bugs

## Prior State

Browser snapshot/projection/clock/diagnostics track had 6 open issues (#2051-#2076) with diagnosed blockers: (1) 'Cleared'-row tombstone not emitted on snapshot clear (violates D4 single-writer contract: ✓ on create, ✗ on clear asymmetry), (2) merge_update_frame not transactional (malformed identity frame could poison baseline projection state), (3) D6 poison-handling inconsistent across read/write paths (reader drops recovered data, writer applies it), (4) provider readiness not initialized for single-provider case. PR #2115 proposed fixes but branch had diverged due to concurrent rebase.

## Trigger

Code review on PR #2115 by agent and codex identified 3 specific bug causes in nmp-browser-runtime/src/runtime.rs pump/projection logic (lines 3399-3410). Concurrent work discovery: another session had rebased the branch onto newer master (merging #2099 registrar rename) and restructured test files, requiring fix-commit transplant and test restructure reconciliation.

## Decision

Fix 4 targeted bugs via code changes: (1) Corrected Cleared-row emitter — now emits real `TombstoneFrame` on snapshot clear (was silently dropping). (2) Made merge_update_frame transactional — builds next projection map + frame_identity in locals, commits atomically to self only after decode_typed_projections fully succeeds (prevents baseline poison on malformed identity frame). (3) Standardized D6 poison handling — reader and projection closure return None on poison (never serialize recovered data); writer recovers via into_inner so writes applied not dropped (consistent contract). (4) Wired provider readiness — CapabilityProviderRegistry::sole_backend() + ready_model() seed slot to ready at start() for single-provider case (multi-provider/lifecycle deferred to #2068). Restructured test monolith into modular tests/{mod,contract,pump,signer}.rs (resolved earlier file-size blocker).

## Consequences

- PR #2115 merged (commit 73ff291b3); all 6 issues (#2051-#2076) closed
- New regression test added for transactional-merge bug (prevents future silent poison)
- Test suite modularization resolved file-size gate blocker
- Native test suite green at 1838 tests (verified no regressions from fixes)
- Snapshot track completed — 23 total issues closed across epic spine/relay/signer-core/snapshot sub-tracks
- Branch divergence (concurrent rebase) resolved cleanly via commit transplant to remote tip

## Open Tail

- Multi-provider and lifecycle-aware provider-readiness deferred as explicit follow-up to #2068

## Evidence

- transcript lines 3399-3410
- transcript lines 3415-3415
- transcript lines 3416-3433
- transcript lines 3437-3488

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-2-snapshot-projection-contract-root-cause-diagnosis.json`](transcripts/2026-06-26-2-snapshot-projection-contract-root-cause-diagnosis.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-2-snapshot-projection-contract-root-cause-diagnosis.json`](transcripts/raw/2026-06-26-2-snapshot-projection-contract-root-cause-diagnosis.json)
