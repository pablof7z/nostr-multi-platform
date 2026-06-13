---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: reversal
status: superseded
subjects:
  - relay-diagnostics
  - nmp-core-projections
  - aim-doctrine
supersedes:
  - 2026-06-13-2-snapshot-path-performance-defects-relay-diagnostics
related_claims: []
source_lines:
  - 5275-5360
  - 5364-5367
captured_at: 2026-06-13T20:13:27Z
---

# Episode: Relay diagnostics must emit raw timestamps, not formatted relative-time strings

## Prior State

relay_diagnostics projection embedded pre-formatted '3s ago'/'42s ago' strings that change every wall-clock second, causing per-tick byte churn on an unchanged projection and forcing the host to re-diff a 'changed' payload forever. This violated aim.md §62 which explicitly forbids format_ago_* inside projection builders.

## Trigger

Time Profiler on a physical iPhone showed relay_diagnostics as the single most prominent named serialization cluster. Investigation found no rationale for the formatted strings — they are a doctrine violation with no sanctioned tradeoff.

## Decision

Switch relay_diagnostics to emit raw timestamps over the wire; shells format relative-time strings at render time. Fix dispatched to Sonnet agent for worktree + PR.

## Consequences

- Eliminates per-tick byte churn on an unchanged projection
- Removes aim.md §62 doctrine violation
- Broader architectural question remains: ADR-0039 rejects host-declared projection subscriptions, so all projections ride every snapshot regardless of whether any view consumes them

## Open Tail

- Per-projection revision gating and host-declared projection subscription sets remain an open design question; the current full-snapshot-every-tick justification (ADR-0037/0039) was critiqued as a false binary but no replacement decision has been made

## Evidence

- transcript lines 5275-5360
- transcript lines 5364-5367

