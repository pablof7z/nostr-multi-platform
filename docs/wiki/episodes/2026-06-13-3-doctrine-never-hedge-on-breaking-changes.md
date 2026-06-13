---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: superseded
subjects:
  - doctrine
  - breaking-changes
  - migration-policy
supersedes: []
related_claims: []
source_lines:
  - 8214-8216
  - 8296-8302
captured_at: 2026-06-13T20:04:54Z
---

# Episode: Doctrine: never hedge on breaking changes — land and upgrade consumers

## Prior State

Assistant posed timing/migration concerns about #1283 (EmbedHost D0 fix), asking whether to start the migration now or hold it for a coordinated/scheduled cut due to breaking changes for external consumer apps (podcast-player, hl, win-the-day) that pin the registry by git rev

## Trigger

Owner explicitly rejected hedging: 'do the right thing, never hedge on breaking change/migration — land it and upgrade the consumer apps by hand each time'

## Decision

Standing project policy: never defer or stage architectural correctness for migration concerns. Land breaking changes and manually upgrade consumer apps (git-rev bumps) each time.

## Consequences

- #1283 EmbedHost migration started immediately with no staged rollout
- Future breaking changes will not be deferred or conditional on consumer coordination
- Saved to memory as standing rule (feedback_do_right_thing_upgrade_consumers.md)

## Open Tail

*(none)*

## Evidence

- transcript lines 8214-8216
- transcript lines 8296-8302

