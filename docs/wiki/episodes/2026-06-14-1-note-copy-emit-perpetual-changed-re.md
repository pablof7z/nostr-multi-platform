---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - projection-rev-presence
  - note-copy-emit
  - rung3-omit
supersedes: []
related_claims: []
source_lines:
  - 8867-8881
  - 8912-8920
  - 8925-8947
captured_at: 2026-06-14T10:45:38Z
---

# Episode: note_copy_emit perpetual-Changed re-emission bug fixed

## Prior State

`note_copy_emit` parked `pending_presence=Changed` on the non-empty arm, so a stable in-flight action (e.g. spinner, lifecycle overlay) re-emitted its full payload as Changed on every 4Hz tick forever, defeating the incremental-omission savings the ADR-0055 ladder exists to produce. Additionally, `ack_action_stage` did not bump any source version, so a partial-ack had no rev advance — masked by the perpetual-Changed override.

## Trigger

Opus review of PR #1390 (R3-S1b) discovered the PR-introduced regression: a steady-state probe showed `action_stages` → Changed ×5 and `action_lifecycle` → Changed ×4 on the PR head, while master correctly settled to Unchanged/absent. The StaleStamp oracle for partial-ack passed only because `pending_presence=Changed` masked the missing rev bump.

## Decision

`note_copy_emit` now parks `pending_presence` ONLY on the Cleared edge (non-empty → empty). The non-empty steady state is left to the rev-vs-last-emit rule so a genuinely-unchanged tick resolves to `Unchanged` and is omitted. `ack_action_stage` now bumps `settlement_enqueue_ver` so partial-ack legitimately advances the rev, keeping the StaleStamp oracle sharp without the override.

## Consequences

- Stable non-empty copy-with-TTL keys (action_stages, action_lifecycle) no longer leak bytes every tick
- Partial-ack is delivered as Changed exactly once then omitted on subsequent unchanged ticks
- New regression test `stable_nonempty_copy_keys_omitted_after_first_changed` prevents recurrence
- StaleStamp oracle for partial-ack remains valid without perpetual-Changed masking

## Open Tail

*(none)*

## Evidence

- transcript lines 8867-8881
- transcript lines 8912-8920
- transcript lines 8925-8947

