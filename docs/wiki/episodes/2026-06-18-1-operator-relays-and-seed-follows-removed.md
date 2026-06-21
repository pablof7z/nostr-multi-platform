---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-core-config
  - operator-relays
  - default-follows
supersedes:
  - 2026-06-18-1-operator-policy-relays-pubkeys-follows-extracted
related_claims: []
source_lines:
  - 50-52
  - 1082-1090
  - 1123-1127
captured_at: 2026-06-18T22:54:46Z
---

# Episode: Operator relays and seed follows removed from NMP core

## Prior State

Hardcoded operator relays and DEFAULT_FOLLOWS (incl fiatjaf) lived in generic NMP layers. Builder accepted implicit relay lists; no type-state forced callers to provide them.

## Trigger

Issue #1493 P9 audit finding: operator-specific config (relays, seed follows) baked into platform-neutral crates, violating layer boundaries.

## Decision

Operator relays and seed follows moved out of NMP to app-level code only. Builder type-state now forces explicit relay provision. PR1 #1550 (merged) is the headline P9 breaking change.

## Consequences

- NMP core is now operator-config-free; consumers must supply relays explicitly
- Breaking change for any code that relied on implicit defaults
- Crate boundary §9 reclassified as part of this PR

## Open Tail

*(none)*

## Evidence

- transcript lines 50-52
- transcript lines 1082-1090
- transcript lines 1123-1127

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-operator-relays-and-seed-follows-removed.json`](transcripts/2026-06-18-1-operator-relays-and-seed-follows-removed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-operator-relays-and-seed-follows-removed.json`](transcripts/raw/2026-06-18-1-operator-relays-and-seed-follows-removed.json)
