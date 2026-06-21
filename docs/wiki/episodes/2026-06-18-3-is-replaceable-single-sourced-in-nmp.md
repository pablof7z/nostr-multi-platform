---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - is-replaceable
  - nmp-kinds
  - nmp-core
supersedes:
  - 2026-06-18-2-single-source-is-replaceable-and-kind
  - 2026-06-18-2-single-source-is-replaceable-in-nmp
related_claims: []
source_lines:
  - 27-28
  - 853-869
captured_at: 2026-06-18T21:02:14Z
---

# Episode: is_replaceable single-sourced in nmp-kinds after 6× divergence

## Prior State

is_replaceable was defined ≥6 times across crates with divergent answers. nmp-core's copy returned true for ALL kinds 0..=19999 (claiming kind:1/6/7 are replaceable — opposite of nostr::Kind and nmp-store). is_parameterized_replaceable wrongly included ephemeral 20000-29999 as addressable. The only call site bound results to underscore-prefixed variables, so no visible wrong decision yet, but event_cycle_key in nmp-content-fixtures DID use the buggy predicate.

## Trigger

P3 audit finding: divergent is_replaceable implementations flagged as a correctness bug. Codex review confirmed canonical NIP-01 = {0,3,41} ∪ [10000,20000) and addressable = [30000,40000).

## Decision

Single canonical implementation placed in nmp-kinds (Layer 0, zero-dep — no nostr dependency added). Both predicates (is_replaceable and is_parameterized_replaceable) live there. nmp-core now pub uses from nmp-kinds. The buggy nmp-core copy is deleted (0 defs remain). kind-const consolidation also landed (KIND_SHORT_NOTE → KIND_SHORT_TEXT_NOTE, marmot/nip60 kind literals consolidated).

## Consequences

- Future kind-predicate changes edit one place
- nmp-kinds stays at Layer 0 with no new dependencies
- is_parameterized_replaceable no longer falsely includes ephemeral 20000-29999
- ops.rs literal consolidation was reverted because it would have pushed the 500-LOC file over its size baseline

## Open Tail

*(none)*

## Evidence

- transcript lines 27-28
- transcript lines 853-869

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-is-replaceable-single-sourced-in-nmp.json`](transcripts/2026-06-18-3-is-replaceable-single-sourced-in-nmp.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-is-replaceable-single-sourced-in-nmp.json`](transcripts/raw/2026-06-18-3-is-replaceable-single-sourced-in-nmp.json)
