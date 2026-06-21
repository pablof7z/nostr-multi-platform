---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - nip17
  - dm-relay
  - relay-bypass-selection
  - explicit-targets
supersedes: []
related_claims: []
source_lines:
  - 31-32
  - 2016-2048
captured_at: 2026-06-19T11:35:40Z
---

# Episode: NIP-17 DM relay bypass omission fixed — prevents silent DM relay pruning

## Prior State

Nip17DmRelay was omitted from relay_bypasses_selection, meaning DM inbox relays could be pruned silently under large follow sets. DMs were being dropped without user awareness.

## Trigger

#1493 audit P7 identified the likely bug; user directed agents to fix P7 as a critical correctness item.

## Decision

Nip17DmRelay added to relay_bypasses_selection so DM inbox relays are never pruned regardless of follow-set size.

## Consequences

- DM relay inbox delivery is now reliable under large follow sets
- Follow-up #1538 filed to unify the dead explicit_targets publish seam

## Open Tail

- #1538 tracks explicit_targets publish-path unification

## Evidence

- transcript lines 31-32
- transcript lines 2016-2048

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-3-nip-17-dm-relay-bypass-omission.json`](transcripts/2026-06-19-3-nip-17-dm-relay-bypass-omission.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-3-nip-17-dm-relay-bypass-omission.json`](transcripts/raw/2026-06-19-3-nip-17-dm-relay-bypass-omission.json)
