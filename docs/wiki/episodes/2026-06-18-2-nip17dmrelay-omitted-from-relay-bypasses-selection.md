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
  - dm-routing
supersedes:
  - 2026-06-18-3-p7-correctness-nip-17-dm-relay
related_claims: []
source_lines:
  - 225-240
  - 370-382
  - 604-614
captured_at: 2026-06-18T19:42:43Z
---

# Episode: Nip17DmRelay omitted from relay_bypasses_selection — DM inbox relays silently pruned

## Prior State

relay_bypasses_selection in nmp-planner/selection.rs omitted RoutingSource::Nip17DmRelay. A gift-wrapped DM inbox relay (Case C) carried an empty-author wildcard sub-shape → zero coverage score → never picked by greedy → survived only via the budget-bounded backfill loop, which stops once max_connections is consumed by NIP-65 outbox relays. Under a large follow set, the DM inbox relay was silently pruned and the user stopped receiving DMs.

## Trigger

#1493 audit finding P7 identified the bypass omission; codex-design-first verification confirmed it as a real correctness bug.

## Decision

Add RoutingSource::Nip17DmRelay to the bypass predicate (alongside Hint/Provenance/AppRelay). Landed in PR #1532 (squash commit b6a2df3be).

## Consequences

- DM inbox relays (kind:10050) are now preserved under large follow sets
- 3 regression tests in selection/dm_relay_tests.rs (fail without bypass, pass with it)
- NIP-29 PublishPlan left untouched — it correctly uses the live PublishTarget::Explicit seam

## Open Tail

*(none)*

## Evidence

- transcript lines 225-240
- transcript lines 370-382
- transcript lines 604-614

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-nip17dmrelay-omitted-from-relay-bypasses-selection.json`](transcripts/2026-06-18-2-nip17dmrelay-omitted-from-relay-bypasses-selection.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-nip17dmrelay-omitted-from-relay-bypasses-selection.json`](transcripts/raw/2026-06-18-2-nip17dmrelay-omitted-from-relay-bypasses-selection.json)
