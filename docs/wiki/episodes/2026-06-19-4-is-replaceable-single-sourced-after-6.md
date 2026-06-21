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
  - 2026-06-18-4-divergent-is-replaceable-consolidated-to-single
related_claims: []
source_lines:
  - 19-48
  - 1965-1966
captured_at: 2026-06-19T00:46:24Z
---

# Episode: is_replaceable single-sourced after 6× divergent definitions

## Prior State

`is_replaceable` was defined ≥6 times across crates with divergent answers: nmp-core said kind:1/6/7 are replaceable; the nostr crate said false for all. This created a real correctness bug where replaceable events could be treated as non-replaceable.

## Trigger

P3 finding in #1493 audit: divergent is_replaceable definitions with real correctness implications.

## Decision

Single-sourced `is_replaceable` in nmp-kinds; all other definitions removed. A latent ephemeral-kind bug was also found and fixed in the same change.

## Consequences

- Replaceability logic is now consistent across the entire codebase
- Future NIP additions only need to update one canonical location
- The per-NIP branching pattern (classify_kind tables, NIP-specific decode in generic layers) was removed alongside this fix

## Open Tail

*(none)*

## Evidence

- transcript lines 19-48
- transcript lines 1965-1966

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-4-is-replaceable-single-sourced-after-6.json`](transcripts/2026-06-19-4-is-replaceable-single-sourced-after-6.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-4-is-replaceable-single-sourced-after-6.json`](transcripts/raw/2026-06-19-4-is-replaceable-single-sourced-after-6.json)
