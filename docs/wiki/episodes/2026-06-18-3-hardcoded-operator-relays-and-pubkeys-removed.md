---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: reversal
status: superseded
subjects:
  - hardcoded-relays
  - hardcoded-pubkeys
  - default-follows
supersedes:
  - 2026-06-18-1-operator-relays-and-seed-follows-removed
related_claims: []
source_lines:
  - 19-48
  - 50-52
captured_at: 2026-06-18T23:05:39Z
---

# Episode: Hardcoded operator relays and pubkeys removed from NMP core to app-level only

## Prior State

Hardcoded operator relays and pubkeys (including DEFAULT_FOLLOWS containing fiatjaf) were embedded in generic/core library layers of NMP, making them shared infrastructure rather than app-specific configuration.

## Trigger

User explicit directive (line 50): "hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself." Reinforced by #1493 audit finding P9 identifying hardcoded operator relays/pubkeys in generic layers.

## Decision

All hardcoded relays and pubkeys are removed from NMP core/generic layers and placed exclusively in app-level code. Core libraries must not contain operator-specific configuration.

## Consequences

- PR #1550 merged: relays and pubkeys extracted from core to app shells
- Establishes a clear boundary: operator configuration is an app concern, not a platform concern
- Future hardcoded config in core will be caught by doctrine lint or code review

## Open Tail

*(none)*

## Evidence

- transcript lines 19-48
- transcript lines 50-52

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-hardcoded-operator-relays-and-pubkeys-removed.json`](transcripts/2026-06-18-3-hardcoded-operator-relays-and-pubkeys-removed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-hardcoded-operator-relays-and-pubkeys-removed.json`](transcripts/raw/2026-06-18-3-hardcoded-operator-relays-and-pubkeys-removed.json)
