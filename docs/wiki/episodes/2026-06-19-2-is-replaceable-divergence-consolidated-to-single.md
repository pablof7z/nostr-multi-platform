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
  - nmp-core
  - nostr-crate
supersedes:
  - 2026-06-19-4-is-replaceable-divergent-definitions-consolidated-to
related_claims: []
source_lines:
  - 27-28
  - 2016-2048
captured_at: 2026-06-19T11:35:40Z
---

# Episode: is_replaceable divergence consolidated to single source in nmp-kinds

## Prior State

is_replaceable was defined ≥6 times with divergent answers: nmp-core said kind:1/6/7 are replaceable; the nostr crate said false. This was a correctness bug — different parts of the system disagreed on which events are replaceable.

## Trigger

#1493 audit P3 flagged divergent is_replaceable definitions as a correctness bug; user directed agents to fix P3 as a critical item requiring codex design review first.

## Decision

All is_replaceable definitions consolidated to a single canonical source in nmp-kinds. Divergent copies removed.

## Consequences

- Single source of truth for event replaceability semantics
- Eliminates the class of bugs where one crate treats an event as replaceable while another treats it as ephemeral
- A latent ephemeral-event bug was also found and fixed during consolidation

## Open Tail

*(none)*

## Evidence

- transcript lines 27-28
- transcript lines 2016-2048

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-2-is-replaceable-divergence-consolidated-to-single.json`](transcripts/2026-06-19-2-is-replaceable-divergence-consolidated-to-single.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-2-is-replaceable-divergence-consolidated-to-single.json`](transcripts/raw/2026-06-19-2-is-replaceable-divergence-consolidated-to-single.json)
