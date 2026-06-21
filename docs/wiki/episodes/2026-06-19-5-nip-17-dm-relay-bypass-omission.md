---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - nip17
  - relay-bypass-selection
  - dm-inbox-relay
supersedes: []
related_claims: []
source_lines:
  - 31-32
captured_at: 2026-06-19T00:46:24Z
---

# Episode: NIP-17 DM relay bypass omission causing silent relay pruning

## Prior State

Nip17DmRelay was omitted from `relay_bypasses_selection`, which meant DM inbox relays could be silently pruned under large follow sets — users would lose DM delivery without knowing.

## Trigger

P7 finding in #1493 audit: likely bug — DM inbox relay silently pruned.

## Decision

Added Nip17DmRelay to the relay bypass selection so DM relay addresses survive relay pruning.

## Consequences

- DM relay addresses are no longer lost during relay selection
- Follow-up #1538 filed for broader explicit_targets publish-path unification

## Open Tail

- #1538 — unify the dead explicit_targets publish seam

## Evidence

- transcript lines 31-32

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-5-nip-17-dm-relay-bypass-omission.json`](transcripts/2026-06-19-5-nip-17-dm-relay-bypass-omission.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-5-nip-17-dm-relay-bypass-omission.json`](transcripts/raw/2026-06-19-5-nip-17-dm-relay-bypass-omission.json)
