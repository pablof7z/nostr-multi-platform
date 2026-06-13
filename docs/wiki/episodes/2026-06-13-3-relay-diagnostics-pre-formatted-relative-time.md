---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: product
status: superseded
subjects:
  - relay-diagnostics
  - projection-format
  - aim-md-doctrine
supersedes:
  - 2026-06-13-2-relay-diagnostics-must-emit-raw-timestamps
related_claims: []
source_lines:
  - 5352-5358
  - 5625-5626
captured_at: 2026-06-13T20:33:24Z
---

# Episode: Relay-diagnostics pre-formatted relative-time strings removed (aim.md §62 violation)

## Prior State

relay_diagnostics embedded pre-formatted '3s ago' / '42s ago' relative-time strings (relay_diagnostics.rs:95-100), changing every wall-clock second, guaranteeing the projection's bytes differed every tick even when nothing real happened. This directly violated aim.md §62 which forbids format_ago_* inside projection builders.

## Trigger

Time Profiler investigation identified relay_diagnostics verify+serialize as the single most prominent named cluster; code audit confirmed the per-second churn and the §62 violation with no rationale found.

## Decision

Ship raw timestamps over the wire; UIs format relative times at render time. PR #1332 (fix/relay-diagnostics-raw-timestamps).

## Consequences

- Eliminates per-second projection churn that poisoned any per-projection change-gate
- Brings relay_diagnostics into compliance with aim.md §62
- Composes with the broader incremental-emission redesign (ADR-0053)

## Open Tail

*(none)*

## Evidence

- transcript lines 5352-5358
- transcript lines 5625-5626

