---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - is-replaceable
  - nmp-kinds
  - kind-fragmentation
supersedes:
  - 2026-06-19-4-is-replaceable-single-sourced-after-6
related_claims: []
source_lines:
  - 27-28
  - 1965-1966
  - 2026-2027
captured_at: 2026-06-19T06:25:53Z
---

# Episode: is_replaceable: divergent definitions consolidated to single source in nmp-kinds

## Prior State

`is_replaceable` was defined ≥6 times across crates with DIVERGENT answers: nmp-core said kind:1/6/7 are replaceable; the nostr crate said false. This caused a correctness bug where events could be misclassified.

## Trigger

P3 audit finding (one of the highest-value actionable items); also a named correctness bug in the #1493 report

## Decision

Consolidated `is_replaceable` to a single source of truth in `nmp-kinds`. All other definitions removed. Ephemeral classification bug also fixed in the same consolidation.

## Consequences

- Single canonical definition eliminates divergent replaceable/volatile classification
- Correctness fix: events previously misclassified by the nostr crate's wrong definition now handled correctly
- Future NIP kind additions only need updating nmp-kinds

## Open Tail

*(none)*

## Evidence

- transcript lines 27-28
- transcript lines 1965-1966
- transcript lines 2026-2027

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-4-is-replaceable-divergent-definitions-consolidated-to.json`](transcripts/2026-06-19-4-is-replaceable-divergent-definitions-consolidated-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-4-is-replaceable-divergent-definitions-consolidated-to.json`](transcripts/raw/2026-06-19-4-is-replaceable-divergent-definitions-consolidated-to.json)
