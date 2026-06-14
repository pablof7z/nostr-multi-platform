---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - s6-oracle-fail-closed
  - absent-key-whitelist
supersedes: []
related_claims: []
source_lines:
  - 9602-9608
  - 9655-9693
captured_at: 2026-06-14T12:34:35Z
---

# Episode: Byte-identity oracle hardened to fail-closed on unexpected absent keys

## Prior State

The capstone byte-identity oracle compared end-states between Phase A (full) and Phase B (omitted), but downgraded absent keys to 'informational only' — a latent hole where a future omit bug dropping a needed Tier-2 row would silently pass instead of failing

## Trigger

Opus review identified that 'absent key → informational' means the oracle cannot distinguish kernel nondeterminism from a real omission bug; a future Tier-2 omission regression would pass silently

## Decision

Made the oracle fail-closed: only two whitelisted Tier-1 keys (claimed_event_embeds, nip46_onboarding) may be absent; any other dropped key or payload mismatch now hard-fails the capstone. Also corrected the docstring from 'every tick' to 'end-state' (the oracle compares final frames, not per-tick)

## Consequences

- Future omission bugs that drop needed Tier-2 rows will be caught by the capstone rather than silently passing
- The whitelist must be updated when new always-Changed Tier-1 projections are registered
- Per-tick oracle comparison remains a follow-on strengthening, not yet implemented

## Open Tail

- Consider driving both phases from the same deterministic kernel state for a stronger oracle (currently two independent kernels, nondeterministic key differences handled by whitelist)

## Evidence

- transcript lines 9602-9608
- transcript lines 9655-9693

