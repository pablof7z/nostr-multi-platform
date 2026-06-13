---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: product
status: active
subjects:
  - relay-diagnostics-projection
  - projection-content-format
  - aim-md-62
supersedes:
  - 2026-06-13-3-relay-diagnostics-pre-formatted-relative-time
related_claims: []
source_lines:
  - 5288-5289
  - 5356-5358
  - 5364-5368
  - 5384-5389
captured_at: 2026-06-13T21:09:24Z
---

# Episode: Prohibit pre-formatted relative-time strings in projection builders

## Prior State

relay_diagnostics.rs embedded pre-formatted "3s ago" / "42s ago" strings in the projection builder (relay_diagnostics.rs:95-100), causing the projection bytes to differ every wall-clock second even when no real state changed — guaranteeing the host re-diffs a changed payload forever.

## Trigger

Performance investigation identified the relative-time strings as the top-named waste cluster in the time profile (RelayDiagnosticsRow verify+serialize). Further audit revealed this violates aim.md §62, which explicitly forbids format_ago_* inside projection builders.

## Decision

Ship raw timestamps over the wire; let the UI layer format relative time at render time. This is both a performance fix (kills per-second projection churn) and a doctrine enforcement (§62 compliance).

## Consequences

- relay_diagnostics projection no longer flips dirty every second — eliminates the single most prominent per-tick re-encode cluster
- Establishes that projection builders must emit raw data, not presentation-formatted strings — a rule that was already written (§62) but was being violated
- Composes with ADR-0053 incremental emission: without this fix, per-projection change-gating would be defeated by projections that churn regardless of real state changes

## Open Tail

- PR #1332 awaiting Opus review and merge

## Evidence

- transcript lines 5288-5289
- transcript lines 5356-5358
- transcript lines 5364-5368
- transcript lines 5384-5389

