---
type: episode-card
date: 2026-07-04
session: d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform--claude-worktrees-fix-2962-flaky-auto-arm/d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5.jsonl
salience: architecture
status: active
subjects:
  - signer-onboarding-projection
  - issue-2993
  - nip-55
  - signer-state
supersedes: []
related_claims: []
source_lines:
  - 395-398
captured_at: 2026-07-04T12:25:02Z
---

# Episode: Signer onboarding projection split deferred — duplicate-path risk with existing projections

## Prior State

Issue #2993 proposed splitting NIP-55 onboarding out of signer_state into its own projection, treating it as cleanup debt near v1.

## Trigger

Opus agent found that bunker_handshake and nip46_onboarding projections already exist and may overlap with a NIP-55 onboarding projection. Minting a third without reconciling risks a duplicate-path violation — a design decision, not a contained change.

## Decision

Defer the split to post-v1. Ship the existing #2976 pending_signer_onboarding bridge at v1 (it's a single documented Option<SignerStateDto> field, cleared on signer add, with unchanged wire/FFI shape). The extraction requires reconciliation of three onboarding projection paths first.

## Consequences

- signer_state remains a pure recomputed output (project_signer_state sole writer, D4 invariant preserved)
- Three onboarding projections (bunker_handshake, nip46_onboarding, NIP-55) must be reconciled before any split
- Published wire/FFI shape unchanged for v1 — no shell depends on the split

## Open Tail

- Reconciliation of onboarding projection paths is a post-v1 design task

## Evidence

- transcript lines 395-398

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-04-3-signer-onboarding-projection-split-deferred-duplicate.json`](transcripts/2026-07-04-3-signer-onboarding-projection-split-deferred-duplicate.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-04-3-signer-onboarding-projection-split-deferred-duplicate.json`](transcripts/raw/2026-07-04-3-signer-onboarding-projection-split-deferred-duplicate.json)
