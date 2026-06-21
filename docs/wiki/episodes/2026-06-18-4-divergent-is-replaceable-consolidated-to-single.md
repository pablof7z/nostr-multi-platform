---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - is-replaceable
  - kind-classification
  - divergent-definitions
supersedes:
  - 2026-06-18-3-kind-predicates-canonical-source-in-nmp
related_claims: []
source_lines:
  - 27-28
  - 36-37
captured_at: 2026-06-18T23:05:39Z
---

# Episode: Divergent is_replaceable consolidated to single canonical definition

## Prior State

is_replaceable was defined ≥6 times across the codebase with divergent answers: nmp-core says kind:1/6/7 are replaceable, while the nostr crate says false for all. No single canonical source of truth existed for this domain-critical classification.

## Trigger

#1493 audit finding P3 (ranked P3): fragmentation with a real correctness bug — the divergent answers mean events could be incorrectly treated as replaceable or non-replaceable depending on which code path was consulted.

## Decision

Consolidate is_replaceable to a single canonical definition in nmp-core. All other definitions are replaced by calls to the canonical source. PR #1534 merged as the P3 kind-fragmentation fix.

## Consequences

- Single source of truth for replaceability classification prevents future divergence
- All code paths now consult the same definition, eliminating the correctness bug
- Sets precedent: kind classification must go through canonical sources, not ad-hoc local definitions

## Open Tail

*(none)*

## Evidence

- transcript lines 27-28
- transcript lines 36-37

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-4-divergent-is-replaceable-consolidated-to-single.json`](transcripts/2026-06-18-4-divergent-is-replaceable-consolidated-to-single.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-4-divergent-is-replaceable-consolidated-to-single.json`](transcripts/raw/2026-06-18-4-divergent-is-replaceable-consolidated-to-single.json)
