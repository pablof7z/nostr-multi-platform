---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: active
subjects:
  - is-replaceable
  - event-semantics
  - nmp-kinds
supersedes:
  - 2026-06-19-2-is-replaceable-divergence-consolidated-to-single
related_claims: []
source_lines:
  - 27-28
  - 2026-2027
captured_at: 2026-06-19T11:51:39Z
---

# Episode: is_replaceable single-sourced in nmp-kinds

## Prior State

is_replaceable defined ≥6 times across crates with divergent answers: nmp-core said kind:1/6/7 are replaceable; the nostr crate said false — a correctness bug affecting event handling.

## Trigger

#1493 audit P3 finding: fragmentation with real correctness divergence.

## Decision

Single-source is_replaceable in nmp-kinds; all consumers point to the canonical definition.

## Consequences

- One canonical answer for event replaceability
- Ephemeral-event bug also discovered and fixed in the same pass
- Merged as #1534

## Open Tail

*(none)*

## Evidence

- transcript lines 27-28
- transcript lines 2026-2027

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-3-is-replaceable-single-sourced-in-nmp.json`](transcripts/2026-06-19-3-is-replaceable-single-sourced-in-nmp.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-3-is-replaceable-single-sourced-in-nmp.json`](transcripts/raw/2026-06-19-3-is-replaceable-single-sourced-in-nmp.json)
