---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - nip17
  - relay-selection
  - explicit-targets
supersedes:
  - 2026-06-18-3-nip-17-dm-inbox-relay-bypass
related_claims: []
source_lines:
  - 31-32
  - 820-821
captured_at: 2026-06-18T21:02:14Z
---

# Episode: NIP-17 DM-inbox relay pruning correctness bug fixed

## Prior State

Nip17DmRelay was omitted from relay_bypasses_selection, meaning DM inbox relays could be silently pruned during relay selection — a real correctness bug causing potential message loss.

## Trigger

P7 audit finding: likely bug — Nip17DmRelay omitted from relay_bypasses_selection → DM inbox relay can be pruned silently.

## Decision

Added Nip17DmRelay to the relay selection bypass list so DM inbox relays are never pruned. Merged as #1532.

## Consequences

- DM inbox relays are now preserved during relay selection
- Follow-up #1538 filed for unifying the dead explicit_targets vs live PublishTarget::Explicit
- Future relay-type additions must be added to the bypass list

## Open Tail

- #1538 (explicit_targets unification) still open

## Evidence

- transcript lines 31-32
- transcript lines 820-821

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-5-nip-17-dm-inbox-relay-pruning.json`](transcripts/2026-06-18-5-nip-17-dm-inbox-relay-pruning.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-5-nip-17-dm-inbox-relay-pruning.json`](transcripts/raw/2026-06-18-5-nip-17-dm-inbox-relay-pruning.json)
