---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-kinds
  - nmp-core
  - is_replaceable
supersedes:
  - 2026-06-18-2-p3-correctness-is-replaceable-has-divergent
related_claims: []
source_lines:
  - 27-28
  - 774-788
captured_at: 2026-06-18T20:12:30Z
---

# Episode: Single-source is_replaceable and kind-const consolidation

## Prior State

is_replaceable was defined ≥6 times with divergent answers — nmp-core said kind:1/6/7 replaceable; the nostr crate said false for all kinds.

## Trigger

Issue #1493 audit (P3) found the divergence as a correctness bug: events could be incorrectly treated as replaceable or ephemeral depending on which definition was consulted.

## Decision

Consolidated is_replaceable into a single source of truth in nmp-kinds, removed all divergent copies, and unified kind constants.

## Consequences

- All consumers now use one authoritative definition, eliminating the correctness divergence.
- Future NIP additions that define new replaceable kinds only need to update one location.

## Open Tail

*(none)*

## Evidence

- transcript lines 27-28
- transcript lines 774-788

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-single-source-is-replaceable-and-kind.json`](transcripts/2026-06-18-2-single-source-is-replaceable-and-kind.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-single-source-is-replaceable-and-kind.json`](transcripts/raw/2026-06-18-2-single-source-is-replaceable-and-kind.json)
