---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - rung3-omit-unchanged
  - cleared-signal-synthesis
  - note-copy-emit
  - projection-manifest
supersedes:
  - 2026-06-14-1-adr-0055-omit-unchanged-cleared-signal
related_claims: []
source_lines:
  - 8673-8680
  - 8697-8772
  - 8837-8910
  - 8922-8948
captured_at: 2026-06-14T09:02:27Z
---

# Episode: Cleared-signal completeness for incremental emission (#1390)

## Prior State

R3-S1's omit_unchanged only iterated present typed rows, so conditionally-present projections (action_results, signed_events, action_stages, action_lifecycle) that drain from non-empty to empty produced a manifest Cleared entry but no synthesized Cleared row for incremental hosts — causing silent stale UI (spinners never dismiss, signed-event continuations replay every tick) the moment incremental_apply was enabled.

## Trigger

R9 adversarial audit (26-agent) filed #1390 as a hard blocker before incremental_apply could be flipped on: 9 confirmed findings (5 HIGH / 3 MED / 1 LOW) showing the Cleared-signal gap.

## Decision

Two-part fix: (1) inverse-pass synthesis in omit_unchanged — for every manifest key not already in the present-row output, synthesize a payload-less Cleared row if the manifest state is Cleared (always) or Changed+conditional-key (defensive belt); hard-assert on any other Changed-but-absent key so producer bugs aren't masked. (2) note_copy_emit edge machine for non-drain trackers (action_stages/action_lifecycle) — mirrors note_drain_emit but parks pending_presence only on the Cleared edge (non-empty→empty), leaving steady-state to the rev-vs-last-emit rule. Review-confirmed refinement: ack_action_stage bumps settlement_enqueue_ver so partial-ack legitimately advances rev (the perpetual-Changed park was masking this missing bump).

## Consequences

- Incremental hosts now receive exactly-one Cleared signal when a conditional projection drains, clearing stale UI without re-emission
- Steady-state non-empty projections resolve to Unchanged/omitted (no byte leak) — the entire savings purpose of ADR-0055 is preserved
- ack_action_stage partial-ack correctly delivers Changed exactly once via legitimate rev advance
- Regression test proven genuinely non-vacuous: 5/6 cases fail on master, all pass on the fix
- declare_incremental_apply returns i32 error codes (0/1/2/-1) instead of void+debug_assert, hardening the FFI gate
- Lock coalesced: incremental_apply_state reads both flags under one guard

## Open Tail

- R3-S3 (iOS ProjectionCache interposer) will be the first host to actually enable incremental_apply
- Finding 4 (host-side clear reorder-guard) deferred to S3 interposer

## Evidence

- transcript lines 8673-8680
- transcript lines 8697-8772
- transcript lines 8837-8910
- transcript lines 8922-8948

