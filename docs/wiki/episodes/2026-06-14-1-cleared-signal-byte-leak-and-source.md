---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - projection-rev-presence
  - omit-unchanged
  - cleared-signal
supersedes:
  - 2026-06-14-1-adr-0055-r3-s1b-cleared-signal
  - 2026-06-14-1-note-copy-emit-perpetual-changed-re
  - 2026-06-14-1-note-copy-emit-perpetual-changed-byte
related_claims: []
source_lines:
  - 8922-8968
captured_at: 2026-06-14T11:34:03Z
---

# Episode: Cleared-signal byte leak and source-version bump (R3-S1b)

## Prior State

note_copy_emit parked pending_presence as Changed on every tick (perpetual-Changed override), and ack_action_stage did not bump settlement_enqueue_ver on partial-ack content edits. Rung 3's omit-Unchanged would suppress Cleared signals (non-empty→empty edge), causing incremental_apply hosts to cache stale UI.

## Trigger

Investigation of #1390 found 9 findings: omit-Unchanged drops Cleared signals. Root cause was the perpetual-Changed override masking a missing source-version bump on partial ack.

## Decision

Fix 1: note_copy_emit now parks presence only on the Cleared edge (non-empty→empty); steady state governed by rev-vs-last-emit. Fix 2: ack_action_stage bumps settlement_enqueue_ver on partial-ack content edit (the hidden dependency the leak was masking). Fix 3: corrected NmpCore.h return-code doc. Regression tests added that fail on reintroduction.

## Consequences

- Rung 3 omit-Unchanged now correctly emits Cleared signals and resolves steady-state keys to Unchanged
- Both mechanisms coexist: ttl_expiry_ver on Cleared edge, settlement_enqueue_ver on partial-ack
- New oracle-gated regression tests prevent silent recurrence

## Open Tail

*(none)*

## Evidence

- transcript lines 8922-8968

