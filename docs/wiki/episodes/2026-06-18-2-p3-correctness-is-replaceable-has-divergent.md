---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-kinds
  - is-replaceable
  - kind-classification
supersedes: []
related_claims: []
source_lines:
  - 27-28
  - 36-37
  - 50-52
  - 151-152
captured_at: 2026-06-18T18:34:07Z
---

# Episode: P3 correctness: is_replaceable has divergent definitions across crates

## Prior State

is_replaceable was defined ≥6 times across the codebase with divergent answers: nmp-core says kind:1/6/7 are replaceable, but the nostr crate says false for the same kinds. This means event processing, tombstone logic, and relay publishing may behave differently depending on which definition is consulted.

## Trigger

P3 finding in the 25-agent audit (issue #1493) flagged the divergence as a correctness bug; user designated it as a critical item requiring codex design-first before implementation.

## Decision

is_replaceable must have a single canonical source of truth in nmp-kinds; all other definitions are to be eliminated and replaced with calls to the canonical definition.

## Consequences

- p3-kind-fragmentation agent owns nmp-kinds and kind consts across all crates exclusively
- Must not touch nmp-store/lmdb (owned by session 10f152 / epic #1523)
- Kind classification logic that was scattered across crates consolidates into one owned module

## Open Tail

- Agent must use codex design-first before implementing to determine the correct canonical semantics (which kinds are replaceable per NIP spec)
- Downstream consumers of the divergent definitions will need call-site updates

## Evidence

- transcript lines 27-28
- transcript lines 36-37
- transcript lines 50-52
- transcript lines 151-152

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-p3-correctness-is-replaceable-has-divergent.json`](transcripts/2026-06-18-2-p3-correctness-is-replaceable-has-divergent.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-p3-correctness-is-replaceable-has-divergent.json`](transcripts/raw/2026-06-18-2-p3-correctness-is-replaceable-has-divergent.json)
