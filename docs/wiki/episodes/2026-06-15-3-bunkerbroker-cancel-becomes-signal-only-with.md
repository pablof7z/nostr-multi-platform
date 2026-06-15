---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-signer-broker
  - bunker-broker-cancel
  - session-generation-guard
supersedes:
  - 2026-06-15-3-signal-only-cancel-with-session-generation
related_claims: []
source_lines:
  - 3661-4011
captured_at: 2026-06-15T17:44:41Z
---

# Episode: BunkerBroker cancel() becomes signal-only with session-generation guard against stale-worker race

## Prior State

BunkerBroker::cancel() called session.relay.shutdown() which performed a synchronous handle.join() on the caller path — blocking the actor thread until the relay worker fully drained, violating the no-caller-blocking invariant.

## Trigger

Codex review (first round) caught that cancel() still joined on the caller path via PoolRelayClient::shutdown (relay_client.rs:289-302). Detaching the join (round 2) introduced a new race: the old worker could later call install_session/install_completed_signer and overwrite the newly-staged session, because start_handshake assumed cancel drained the old worker first.

## Decision

Make cancel() signal-only: uses signal_shutdown() + spawn_reaper for all joins (zero caller-path blocking). Add monotonic generation: AtomicU64 on BunkerBroker — start_handshake/cancel bump it, workers carry their generation stamp, install_session/install_completed_signer reject stale-generation calls. Superseded workers self-clean by tearing down their relay off-path. DNS resolution bounded by running to_socket_addrs on a detached helper thread with TCP_CONNECT_TIMEOUT deadline.

## Consequences

- cancel() never blocks the caller — the actor is always responsive
- Stale workers cannot clobber the active session — generation guard is atomic under the active mutex
- DNS resolution bounded; documented residual for trickling-TLS (per-syscall inactivity timeout, not absolute deadline — rare adversarial case leaks one thread)
- Reaper-join test non-vacuously verified (observer fires only after all .join()s return; replacing join with drop fails the test)

## Open Tail

- Trickling-TLS residual: a full interruptible-socket/total-deadline rewrite is out of D4 scope but tracked as a separate item

## Evidence

- transcript lines 3661-4011
