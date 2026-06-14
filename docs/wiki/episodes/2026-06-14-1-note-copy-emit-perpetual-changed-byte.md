---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - projection-rev-note-copy-emit
  - incremental-emission-cleared-signal
supersedes: []
related_claims: []
source_lines:
  - 8842-8920
captured_at: 2026-06-14T10:26:44Z
---

# Episode: note_copy_emit perpetual-Changed byte leak fix (R3-S1b)

## Prior State

note_copy_emit parked pending_presence=Changed on the non-empty arm, causing stable in-flight actions (spinners/lifecycle overlays) to re-emit their full payload every 4Hz tick forever even when nothing changed — defeating the byte savings the entire ADR-0055 ladder exists to deliver. This perpetual-Changed also masked a missing source-version bump on ack_action_stage (partial-ack content change did not advance the rev).

## Trigger

Opus adversarial review (PR #1393) added a steady-state probe and found action_stages→Changed×5 and action_lifecycle→Changed×4 on unchanged ticks — a PR-introduced regression vs master where stable non-empty correctly settled to Unchanged/absent.

## Decision

1) note_copy_emit now parks pending_presence ONLY on the Cleared (empty) edge; the non-empty steady state is left to the rev-vs-last-emit rule so genuinely-unchanged ticks resolve to Unchanged/omitted. 2) ack_action_stage now bumps settlement_enqueue_ver so partial-ack legitimately advances the rev, removing the dependency on the perpetual-Changed override that was masking it. 3) Added steady-state regression assertion (stable_nonempty_copy_keys_omitted_after_first_changed) that fails if either defect reappears.

## Consequences

- Steady-state non-empty keys correctly settle to Unchanged/omitted — the incremental host realizes the byte savings
- Partial-ack correctly signals Changed exactly once via legitimate rev advancement, not via perpetual presence override
- The StaleStamp oracle remains sharp: removing the override no longer causes false StaleStamp panics on partial-ack
- Regression tests are revert-proof: reintroducing the leak fails at tick 2; removing the rev bump panics with OracleViolation

## Open Tail

*(none)*

## Evidence

- transcript lines 8842-8920

