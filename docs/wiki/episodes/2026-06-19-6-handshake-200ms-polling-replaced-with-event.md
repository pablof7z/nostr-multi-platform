---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - signer-broker-handshake
  - polling-elimination
  - crossbeam-channel
supersedes:
  - 2026-06-18-4-handshake-cancel-polling-loop-replaced-by
  - 2026-06-19-4-event-driven-handshake-replaces-200ms-polling
related_claims: []
source_lines:
  - 19-48
  - 1819-1820
captured_at: 2026-06-19T00:46:24Z
---

# Episode: Handshake 200ms polling replaced with event-driven crossbeam channel

## Prior State

The signer-broker handshake used a 200ms polling loop (D8 anti-pattern from the audit), introducing latency and wasting CPU cycles.

## Trigger

P5 finding in #1493 audit: polling/sleep loops identified as an architectural anti-pattern.

## Decision

Replaced the 200ms poll with event-driven crossbeam channel wiring in nmp-signer-broker's handshake.rs. The handshake now wakes immediately on events rather than polling on a timer.

## Consequences

- Handshake latency eliminated (no more 200ms polling interval)
- CPU cycles no longer wasted on periodic polling
- The crossbeam wiring was sequenced before the nostrconnect perms change (PR1b) since both touch broker/nostrconnect.rs

## Open Tail

*(none)*

## Evidence

- transcript lines 19-48
- transcript lines 1819-1820

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-6-handshake-200ms-polling-replaced-with-event.json`](transcripts/2026-06-19-6-handshake-200ms-polling-replaced-with-event.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-6-handshake-200ms-polling-replaced-with-event.json`](transcripts/raw/2026-06-19-6-handshake-200ms-polling-replaced-with-event.json)
