---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - p7-nip17-routing
  - relay-bypasses-selection
  - nip17-dm-relay
supersedes:
  - 2026-06-18-5-nip-17-dm-inbox-relay-pruning
related_claims: []
source_lines:
  - 31-32
  - 833-839
captured_at: 2026-06-18T21:31:23Z
---

# Episode: NIP-17 DM relay bypass: silent relay pruning bug

## Prior State

Nip17DmRelay was omitted from relay_bypasses_selection, meaning the DM inbox relay could be pruned silently by the relay selection algorithm — a correctness bug causing potential message loss.

## Trigger

#1493 P7 finding: Nip17DmRelay not in bypass set → DM inbox relay prunable.

## Decision

Added Nip17DmRelay to the relay_bypasses_selection set so the DM inbox relay is preserved during selection. Deeper fix filed as #1538 for more comprehensive NIP-17 routing coverage.

## Consequences

- DM inbox relays are no longer silently pruned.
- #1538 filed for broader NIP-17 relay routing improvements beyond the bypass fix.

## Open Tail

- #1538 open for deeper NIP-17 routing work.

## Evidence

- transcript lines 31-32
- transcript lines 833-839

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-7-nip-17-dm-relay-bypass-silent.json`](transcripts/2026-06-18-7-nip-17-dm-relay-bypass-silent.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-7-nip-17-dm-relay-bypass-silent.json`](transcripts/raw/2026-06-18-7-nip-17-dm-relay-bypass-silent.json)
