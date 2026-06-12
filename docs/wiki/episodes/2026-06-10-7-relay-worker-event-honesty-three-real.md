---
type: episode-card
date: 2026-06-10
session: 8db7983d-2852-4213-9b8c-43650a958e7a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/8db7983d-2852-4213-9b8c-43650a958e7a.jsonl
salience: root-cause
status: active
subjects:
  - nmp-network
  - relay-worker
  - keepalive
  - mio
supersedes: []
related_claims: []
source_lines:
  - 1300-1336
captured_at: 2026-06-11T23:11:53Z
---

# Episode: Relay worker event honesty — three real bugs, one disproven

## Prior State

Three latent bugs in relay_worker: (1) silent exit on `ControlDrain::Disconnected` — no terminal event emitted; (2) `ping_sent_at` stamped before the ping reached the wire — if flush blocked, pong-timeout clock ran for a never-sent ping, causing spurious disconnects under write congestion; (3) `set_wants_write = false` on `Flushed` arm cleared pending write interest set by `flush_relay_writes`.

## Trigger

Systematic relay_worker audit found four candidates; three confirmed as real bugs with TDD proof.

## Decision

(1) Emit `RelayEvent::Closed` from the `Disconnected` arm; fire mio `Waker` on upstream sender drop for prompt wake. (2) `step()` no longer stamps `ping_sent_at`; new `on_ping_flushed(now)` called only after `FlushResult::Flushed`. (3) `Flushed` arm no longer assigns `wants_write`. Finding 4 (tungstenite out_buffer data loss) was disproven as expected stream-protocol behavior.

## Consequences

- No more silent worker exits — host always receives a terminal Closed event
- Pong timeout only fires for pings that actually reached the wire — no spurious disconnects under congestion
- Write interest is no longer spuriously cleared when pending writes exist

## Open Tail

*(none)*

## Evidence

- transcript lines 1300-1336

