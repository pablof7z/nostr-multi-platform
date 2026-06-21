---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - p7-nip17-routing
  - relay-bypasses-selection
  - nmp-planner
supersedes:
  - 2026-06-18-1-nip-17-dm-inbox-relays-bypass
related_claims: []
source_lines:
  - 30-31
  - 604-614
captured_at: 2026-06-18T20:25:04Z
---

# Episode: NIP-17 DM-inbox relay bypass for selection pruning

## Prior State

Nip17DmRelay was not included in relay_bypasses_selection, so the NIP-65 optimizer could prune DM inbox relays when a user had a large follow set, silently dropping DMs.

## Trigger

Issue #1493 P7 identified this as a likely correctness bug — DM inbox relay can be pruned silently.

## Decision

Added RoutingSource::Nip17DmRelay to relay_bypasses_selection. DM inbox relays now bypass the NIP-65 selection pruning. +3 regression tests.

## Consequences

- DMs no longer silently dropped under large follow sets
- Relay_pin vs dynamic lookup finding (P7 F1) was judged not-a-defect — correct by design
- NIP-29 PublishPlan was confirmed correct via the live seam

## Open Tail

- Issue #1538 filed: unify dead RoutingContext::explicit_targets vs live PublishTarget::Explicit — two parallel publish-side explicit-relay mechanisms, one dead

## Evidence

- transcript lines 30-31
- transcript lines 604-614

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-nip-17-dm-inbox-relay-bypass.json`](transcripts/2026-06-18-3-nip-17-dm-inbox-relay-bypass.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-nip-17-dm-inbox-relay-bypass.json`](transcripts/raw/2026-06-18-3-nip-17-dm-inbox-relay-bypass.json)
