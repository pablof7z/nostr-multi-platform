---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-planner
  - relay-selection
  - nip17
supersedes:
  - 2026-06-18-2-nip17dmrelay-omitted-from-relay-bypasses-selection
related_claims: []
source_lines:
  - 19-48
  - 370-382
  - 604-613
captured_at: 2026-06-18T20:12:30Z
---

# Episode: NIP-17 DM-inbox relays bypass selection pruning

## Prior State

Nip17DmRelay was omitted from relay_bypasses_selection, so the NIP-65 outbox optimizer could prune kind:10050 DM-inbox relays, silently dropping DMs under large follow sets.

## Trigger

Issue #1493 audit (P7) identified the omission as a correctness bug.

## Decision

Added RoutingSource::Nip17DmRelay to relay_bypasses_selection so DM-inbox relays are never pruned by the outbox optimizer. 3 regression tests added.

## Consequences

- DM inbox relay connectivity is preserved regardless of follow-list size.
- NIP-29 PublishPlan and mailboxes.rs left untouched — triaged as either not-a-defect or needing a separate publish-path lane.
- Follow-up issue #1538 filed to unify the dead RoutingContext::explicit_targets seam with the live PublishTarget::Explicit path.

## Open Tail

- #1538: decide whether to delete dead RoutingContext::explicit_targets or migrate PublishTarget::Explicit through route_publish.

## Evidence

- transcript lines 19-48
- transcript lines 370-382
- transcript lines 604-613

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nip-17-dm-inbox-relays-bypass.json`](transcripts/2026-06-18-1-nip-17-dm-inbox-relays-bypass.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nip-17-dm-inbox-relays-bypass.json`](transcripts/raw/2026-06-18-1-nip-17-dm-inbox-relays-bypass.json)
