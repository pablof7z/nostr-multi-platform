---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - p3-kind-fragmentation
  - is_replaceable
  - nmp-kinds
supersedes: []
related_claims: []
source_lines:
  - 26-27
  - 849-870
captured_at: 2026-06-18T20:25:04Z
---

# Episode: Single-source is_replaceable in nmp-kinds

## Prior State

is_replaceable was defined ≥6 times with divergent answers: nmp-core returned true for ALL kind 0..=19999 (wrongly marking kind:1/6/7 as replaceable), while the nostr crate said false. is_parameterized_replaceable wrongly included ephemeral 20000-29999 as addressable.

## Trigger

Issue #1493 P3 identified the divergence as a real correctness bug — the nmp-core copy was a reachable trap (even though its only call site bound results to _-prefixed vars, nmp-content-fixtures::event_cycle_key DID use it).

## Decision

Both is_replaceable and is_parameterized_replaceable are now single-sourced in nmp-kinds (Layer 0, zero-dep). Canonical NIP-01 predicate: replaceable = {0,3,41} ∪ [10000,20000), addressable = [30000,40000). nmp-core uses pub use only; all other copies removed.

## Consequences

- nmp-content-fixtures::event_cycle_key was using the wrong predicate — now fixed
- An attempted ops.rs literal-consolidation was reverted because it pushed the 500-LOC god-file over its file-size baseline (713→718)
- KIND_MUTE_LIST comment was also stale — fixed literal + comment
- nmp-kinds remains zero-dep (nostr crate NOT added as dependency to Layer 0)

## Open Tail

*(none)*

## Evidence

- transcript lines 26-27
- transcript lines 849-870

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-single-source-is-replaceable-in-nmp.json`](transcripts/2026-06-18-2-single-source-is-replaceable-in-nmp.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-single-source-is-replaceable-in-nmp.json`](transcripts/raw/2026-06-18-2-single-source-is-replaceable-in-nmp.json)
