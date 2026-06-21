---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - signer-broker-handshake
  - polling-elimination
  - crossbeam-migration
supersedes: []
related_claims: []
source_lines:
  - 19-48
  - 1316-1317
  - 1415-1423
captured_at: 2026-06-18T23:24:57Z
---

# Episode: Event-driven handshake cancel replaces 200ms polling loop

## Prior State

nmp-signer-broker used a 200ms polling/sleep loop for handshake cancel detection, violating the D8 doctrine (no polling/sleep loops in agent-owned code).

## Trigger

#1493 P5 finding identified polling/sleep loops as an architectural exception pattern to be eliminated.

## Decision

Replace 200ms poll with crossbeam-channel event-driven handshake cancel. The handshake thread now blocks on a crossbeam Receiver instead of polling, eliminating the sleep loop entirely.

## Consequences

- Three signer-broker files exceeded the 500-line file-size hard cap after crossbeam additions, requiring mechanical code splits (broker.rs → broker/handshake_thread.rs, handshake.rs → handshake/nostrconnect.rs, handshake/tests.rs → tests/{mod,bunker,nostrconnect,units}.rs)
- crossbeam-channel requires cfg-gating for wasm (nmp-signer-broker isn't compiled for wasm, but the workspace wasm build must still parse all crates' type signatures)
- Codex review artifact saved per standing gate (docs/perf/codex-reviews/)

## Open Tail

*(none)*

## Evidence

- transcript lines 19-48
- transcript lines 1316-1317
- transcript lines 1415-1423

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-event-driven-handshake-cancel-replaces-200ms.json`](transcripts/2026-06-18-3-event-driven-handshake-cancel-replaces-200ms.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-event-driven-handshake-cancel-replaces-200ms.json`](transcripts/raw/2026-06-18-3-event-driven-handshake-cancel-replaces-200ms.json)
