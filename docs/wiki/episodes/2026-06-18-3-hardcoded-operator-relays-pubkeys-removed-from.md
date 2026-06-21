---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - hardcoded-config
  - default-relays
  - default-follows
  - nmp-core
supersedes:
  - 2026-06-18-3-hardcoded-operator-relays-and-pubkeys-removed
related_claims: []
source_lines:
  - 50-52
  - 1500-1502
captured_at: 2026-06-18T23:38:04Z
---

# Episode: Hardcoded operator relays/pubkeys removed from NMP core

## Prior State

Hardcoded operator relays/pubkeys (including DEFAULT_FOLLOWS with fiatjaf) existed in generic/core layers of NMP.

## Trigger

Issue #1493 P9 finding; user's explicit directive (line 50): 'hardcoded relays and pubkeys belong ONLY in app level code, not NMP itself'

## Decision

All hardcoded relays and pubkeys removed from nmp-core; moved to app-level configuration (nmp-chirp-config). NMP core never embeds product-default endpoints or follow sets.

## Consequences

- PR #1550 merged: relays/pubkeys out of NMP core
- App-level code (Chirp) supplies defaults; NMP core is product-neutral
- Future config must follow the app-supplied pattern (mirrors nostrconnect_bootstrap_relay slot)

## Open Tail

*(none)*

## Evidence

- transcript lines 50-52
- transcript lines 1500-1502

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-hardcoded-operator-relays-pubkeys-removed-from.json`](transcripts/2026-06-18-3-hardcoded-operator-relays-pubkeys-removed-from.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-hardcoded-operator-relays-pubkeys-removed-from.json`](transcripts/raw/2026-06-18-3-hardcoded-operator-relays-pubkeys-removed-from.json)
