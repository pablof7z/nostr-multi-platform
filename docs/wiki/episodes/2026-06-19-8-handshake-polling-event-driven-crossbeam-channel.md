---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: active
subjects:
  - signer-handshake
  - polling-elimination
supersedes:
  - 2026-06-19-7-signer-handshake-200ms-polling-replaced-with
related_claims: []
source_lines:
  - 29-30
  - 2028-2029
captured_at: 2026-06-19T11:51:39Z
---

# Episode: Handshake polling → event-driven crossbeam channel

## Prior State

Signer handshake cancellation detection used a 200ms polling loop.

## Trigger

#1493 audit P5 finding: polling/sleep loops (D8 doctrine violation).

## Decision

Replaced polling with crossbeam channel event-driven wakeup for handshake cancellation.

## Consequences

- No polling delay; immediate cancellation detection
- P5's #1540 (nip60 doc) + #1547 (crossbeam) merged
- Unused mpsc warning in nmp-core/actor/inbox.rs noted as pre-existing (cfg-native, unrelated to this change)

## Open Tail

*(none)*

## Evidence

- transcript lines 29-30
- transcript lines 2028-2029

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-8-handshake-polling-event-driven-crossbeam-channel.json`](transcripts/2026-06-19-8-handshake-polling-event-driven-crossbeam-channel.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-8-handshake-polling-event-driven-crossbeam-channel.json`](transcripts/raw/2026-06-19-8-handshake-polling-event-driven-crossbeam-channel.json)
