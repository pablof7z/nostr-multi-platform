---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: active
subjects:
  - nip17-dm-relay
  - relay-bypass-selection
  - dm-inbox
supersedes:
  - 2026-06-19-5-nip-17-dm-relay-bypass-omission
  - 2026-06-18-7-nip-17-dm-relay-bypass-silent
related_claims: []
source_lines:
  - 31-32
  - 2029-2030
captured_at: 2026-06-19T06:25:53Z
---

# Episode: NIP-17 DM relay pruning bug: Nip17DmRelay added to relay bypass set

## Prior State

`Nip17DmRelay` was omitted from `relay_bypasses_selection`, causing DM inbox relay to be silently pruned under large follow sets. DMs could be lost without user awareness.

## Trigger

P7 audit finding: 'likely bug — Nip17DmRelay omitted from relay_bypasses_selection → DM inbox relay can be pruned silently'

## Decision

Added Nip17DmRelay to the bypass set so DM inbox relays are never pruned regardless of follow-set size. Filed #1538 for broader explicit_targets publish-path unification.

## Consequences

- DM inbox relay preserved under large follow sets — no more silent DM loss
- Follow-up #1538 filed for unifying the broader explicit_targets publish seam

## Open Tail

- #1538 — unify dead explicit_targets publish seam

## Evidence

- transcript lines 31-32
- transcript lines 2029-2030

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-5-nip-17-dm-relay-pruning-bug.json`](transcripts/2026-06-19-5-nip-17-dm-relay-pruning-bug.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-5-nip-17-dm-relay-pruning-bug.json`](transcripts/raw/2026-06-19-5-nip-17-dm-relay-pruning-bug.json)
