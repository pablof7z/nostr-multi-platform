---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - signer-broker
  - handshake-cancel
  - polling-elimination
  - crossbeam
supersedes:
  - 2026-06-18-3-event-driven-handshake-cancel-replaces-200ms
related_claims: []
source_lines:
  - 1415-1424
captured_at: 2026-06-18T23:38:04Z
---

# Episode: Handshake cancel: polling loop replaced by event-driven channel

## Prior State

Signer-broker handshake cancel detection used a 200ms polling/sleep loop checking for cancellation state.

## Trigger

Issue #1493 P5 identified polling/sleep loops as an architectural anti-pattern; also missing async success-terminal recording causing hung spinners.

## Decision

Replaced 200ms poll with crossbeam-channel event-driven handshake cancel. Signer-broker now receives cancellation signals via channel rather than polling.

## Consequences

- PRs #1540 and #1547 merged; handshake cancel is instant (no 200ms latency)
- crossbeam-channel addition pushed signer-broker files over the 500-line file-size ceiling, requiring a module split (broker.rs→broker/handshake_thread.rs, handshake.rs→handshake/nostrconnect.rs, tests split into submodules)
- crossbeam does not compile for wasm32 — signer-broker is cfg(native)-only, which is correct since it manages OS-native signing sessions

## Open Tail

*(none)*

## Evidence

- transcript lines 1415-1424

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-4-handshake-cancel-polling-loop-replaced-by.json`](transcripts/2026-06-18-4-handshake-cancel-polling-loop-replaced-by.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-4-handshake-cancel-polling-loop-replaced-by.json`](transcripts/raw/2026-06-18-4-handshake-cancel-polling-loop-replaced-by.json)
