---
type: episode-card
date: 2026-06-03
session: b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf.jsonl
salience: architecture
status: active
subjects:
  - ffi-surface-freeze
  - c-abi-naming
  - pd-039-removal
supersedes: []
related_claims: []
source_lines:
  - 800-876
captured_at: 2026-06-11T23:02:15Z
---

# Episode: C-ABI surface reframed as frozen framework API, not migration debt

## Prior State

Named nmp_app_* C-ABI symbols were categorized as migration debt with a bespoke deprecation calendar (PD-039), implying they would be replaced over time by a dispatch_action-based seam.

## Trigger

User directive that named C-ABI symbols are the correct framework API, not temporary debt to be migrated away.

## Decision

Deleted docs/architecture-audit/ffi-deprecation-calendar.md, removed PD-039 from BACKLOG.md, and rewrote plan.md exit criterion #7 from migration-debt framing to a freeze-gate statement: the 54-symbol surface is frozen, new symbols require an ADR, and CI enforces this.

## Consequences

- The nmp_app_* C-ABI surface is now doctrinally permanent — additions require an ADR (ADR-0041 pattern)
- V-68 Stage 2 (hardcoded social kinds {1,6} in open_* functions) is tracked as the only real remaining issue, in #911
- The CI freeze gate changes from a deprecation tracker to a stability gate
- PR #910 had to be rebased to drop a hunk that edited the now-deleted calendar file

## Open Tail

- Four other docs still reference the deleted calendar in prose (not broken links) — may need a follow-up scrub
- Future nmp_app_add_signer_* symbols from PR #908 will need their own ADR before landing

## Evidence

- transcript lines 800-876

