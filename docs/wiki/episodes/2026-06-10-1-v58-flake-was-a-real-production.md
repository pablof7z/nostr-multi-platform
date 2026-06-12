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
  - kqueue
  - edge-triggered-io
supersedes: []
related_claims: []
source_lines:
  - 1078-1101
captured_at: 2026-06-11T23:11:53Z
---

# Episode: v58 "flake" was a real production bug — edge-triggered poll-event loss

## Prior State

v58_set_backoff_hint_does_not_break_reconnect was classified as a flaky test — re-run and ignore. The production loop checked `if ready.control || ready.writable { continue; }` before `drain_relay_reads`, silently discarding co-arriving EOF/read events on macOS kqueue EV_CLEAR.

## Trigger

TDD audit root-caused the flake: on macOS, mio's kqueue EV_CLEAR (edge-triggered) delivers both EVFILT_READ+EV_EOF and EVFILT_USER in one kevent() batch. The `continue` consumed the control event and re-entered poll, but the EOF transition was already delivered and never re-fired — worker blocked for 60s keepalive timeout.

## Decision

Production code fix: `drain_relay_reads` now executes unconditionally before the `if ready.control || ready.writable { continue; }` check. Test fix: channel-coordinated drop synchronization replaces racy `drop(accept(stream))`-immediately pattern.

## Consequences

- The v58 test failure was a genuine user-visible read-starvation bug, not a test flake
- Both 're-run and ignore' memory entries for this test are retired — future failures mean real regression
- Same race class could theoretically affect any platform using edge-triggered I/O, not just macOS

## Open Tail

*(none)*

## Evidence

- transcript lines 1078-1101

