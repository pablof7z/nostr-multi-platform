---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: active
subjects:
  - nip17-dm-relay
  - relay-bypasses-selection
  - explicit-targets
supersedes:
  - 2026-06-19-3-nip-17-dm-relay-bypass-omission
related_claims: []
source_lines:
  - 31-32
  - 2029-2030
captured_at: 2026-06-19T11:51:39Z
---

# Episode: DM relay pruning correctness bug fixed

## Prior State

Nip17DmRelay was omitted from relay_bypasses_selection, causing DM inbox relays to be silently pruned under large follow sets — DMs dropped without user awareness.

## Trigger

#1493 audit P7 finding: likely bug where DM inbox relay can be pruned silently.

## Decision

Added Nip17DmRelay to the relay bypass set so DM relays are preserved during selection pruning.

## Consequences

- DMs no longer silently dropped under large follow sets
- Merged as #1532
- Follow-up filed: #1538 to unify the dead explicit_targets vs live PublishTarget::Explicit publish seam

## Open Tail

- #1538 — explicit_targets publish-path unification

## Evidence

- transcript lines 31-32
- transcript lines 2029-2030

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-4-dm-relay-pruning-correctness-bug-fixed.json`](transcripts/2026-06-19-4-dm-relay-pruning-correctness-bug-fixed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-4-dm-relay-pruning-correctness-bug-fixed.json`](transcripts/raw/2026-06-19-4-dm-relay-pruning-correctness-bug-fixed.json)
