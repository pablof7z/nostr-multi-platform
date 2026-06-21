---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - p3-kind-fragmentation
  - is-replaceable
  - nmp-kinds
supersedes:
  - 2026-06-18-3-is-replaceable-single-sourced-in-nmp
related_claims: []
source_lines:
  - 25-28
  - 862-869
captured_at: 2026-06-18T21:31:23Z
---

# Episode: Kind predicates: canonical source in nmp-kinds, zero-dep

## Prior State

is_replaceable defined ≥6 times across crates with divergent answers (nmp-core said kind:1/6/7 replaceable; nostr crate said false). is_parameterized_replaceable wrongly included ephemeral kinds 20000-29999.

## Trigger

#1493 P3 finding: divergent is_replaceable is a correctness bug (wrong relay filter behavior, replaceable events treated as ephemeral or vice versa).

## Decision

Both is_replaceable and is_parameterized_replaceable are now canonical NIP-01 predicates in nmp-kinds (single source, zero-dep — no nostr crate dependency at Layer 0). nmp-core just pub use's them. Canonical definitions: replaceable = {0,3,41} ∪ [10000,20000); addressable = [30000,40000). Tests assert the kind:1/6/7 + ephemeral cases.

## Consequences

- Bonus bug caught: is_parameterized_replaceable included ephemeral 20000-29999 — now excluded.
- ops.rs literal-consolidation was reverted because it pushed the 500-LOC god-file over its baseline (713→718, CI file-size failure).
- Marmot key-package u16/u32 split and nip01 KIND_SHORT_NOTE literal consolidated.

## Open Tail

*(none)*

## Evidence

- transcript lines 25-28
- transcript lines 862-869

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-kind-predicates-canonical-source-in-nmp.json`](transcripts/2026-06-18-3-kind-predicates-canonical-source-in-nmp.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-kind-predicates-canonical-source-in-nmp.json`](transcripts/raw/2026-06-18-3-kind-predicates-canonical-source-in-nmp.json)
