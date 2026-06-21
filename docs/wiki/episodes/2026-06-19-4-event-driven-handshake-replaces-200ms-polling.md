---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - signer-broker
  - handshake
  - polling
  - crossbeam
supersedes: []
related_claims: []
source_lines:
  - 1415-1423
captured_at: 2026-06-19T00:18:35Z
---

# Episode: Event-driven handshake replaces 200ms polling in signer-broker

## Prior State

Signer-broker handshake used a 200ms polling loop to detect cancellation/completion.

## Trigger

#1493 P5 audit identified the polling loop as an architectural exception (D8: polling/sleep loops).

## Decision

Replaced with crossbeam-channel event-driven handshake cancellation.

## Consequences

- More responsive handshake — no 200ms latency floor
- File-size ceiling required splitting broker.rs (710→424) and handshake.rs (584→391) into submodules (broker/handshake_thread.rs, handshake/nostrconnect.rs, handshake/tests/{mod,bunker,nostrconnect,units}.rs)
- crossbeam-channel doesn't compile for wasm32 — not an issue since signer-broker isn't compiled for wasm, but stale branches predating the wasm-safe split (#1572) hit false E0432 failures
- Baseline entries for the old monolithic files removed (tightening, not bumping)

## Open Tail

*(none)*

## Evidence

- transcript lines 1415-1423

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-4-event-driven-handshake-replaces-200ms-polling.json`](transcripts/2026-06-19-4-event-driven-handshake-replaces-200ms-polling.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-4-event-driven-handshake-replaces-200ms-polling.json`](transcripts/raw/2026-06-19-4-event-driven-handshake-replaces-200ms-polling.json)
