---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: workflow
status: superseded
subjects:
  - doctrine-resolution
  - needs-decision
  - aim-md
  - zero-debt
  - d0-doctrine
supersedes: []
related_claims: []
source_lines:
  - 8084-8158
captured_at: 2026-06-13T19:35:42Z
---

# Episode: 10 of 11 needs-decision issues already determined by documented direction

## Prior State

11 GitHub issues labeled status:needs-decision were waiting for owner input, blocking implementation.

## Trigger

User bet that most decisions were already implied by documented product direction and instructed a single Opus agent to resolve all 11 against aim.md/plan.md/doctrine/ADRs.

## Decision

10 of 11 resolved without owner input: D0 + thin-shell + zero-debt + single-mechanism-cache-serve + v1-platform-contract dictated the answer. Only #1281 (since=None backfill semantics) genuinely needs owner judgment. Stale labels cleaned; implementation dispatched for #1090, #1202, #1250.

## Consequences

- Establishes that doctrine/aim/plan are sufficient to resolve most 'decisions' without escalation
- Six issues had stale needs-decision labels (post-v1 or already decided: #1008, #999, #967, #980, #920, #1291)
- #1250 and #1202 resolved by zero-debt rule: park dead islands explicitly, never let preview silently always-fail
- Only #1281 remains for owner: exempt since=None from watermark rewrite, or keep T129 as designed

## Open Tail

- #1281 awaits owner's choice on backfill semantics

## Evidence

- transcript lines 8084-8158

