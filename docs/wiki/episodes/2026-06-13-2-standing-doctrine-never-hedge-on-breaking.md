---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: active
subjects:
  - doctrine
  - breaking-change-policy
  - consumer-upgrade
supersedes:
  - 2026-06-13-3-doctrine-never-hedge-on-breaking-changes
related_claims: []
source_lines:
  - 8294-8302
captured_at: 2026-06-13T20:42:49Z
---

# Episode: Standing doctrine: never hedge on breaking changes — upgrade consumers by hand

## Prior State

The AI was asking whether to schedule or delay breaking changes that affect external consumer apps (podcast-player, hl, win-the-day), treating coordinated migration as a blocking concern

## Trigger

Owner's rule of thumb: 'do the right thing, never hedge on breaking change/migration — land it and upgrade the consumer apps by hand each time'

## Decision

Standing policy saved to memory: always land the correct architectural change immediately; never delay or schedule around external-consumer breakage — instead bump consumer git-rev references by hand

## Consequences

- #1283 (EmbedHost migration) started immediately rather than being deferred for coordinated scheduling
- Future breaking-change questions are no longer posed — the answer is always 'proceed now, upgrade consumers manually'
- Applies to any future crate-boundary or contract change that pins external consumers

## Open Tail

*(none)*

## Evidence

- transcript lines 8294-8302

