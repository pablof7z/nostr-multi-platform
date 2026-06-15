---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - bunker-broker-cancel
  - session-generation-guard
  - detached-reaper
  - connect-timeout-bounding
supersedes:
  - 2026-06-15-3-bunkerbroker-cancel-signal-only-session-generation
related_claims: []
source_lines:
  - 3560-4010
captured_at: 2026-06-15T17:37:54Z
---

# Episode: Signal-only cancel with session-generation guard

## Prior State

BunkerBroker::cancel() joined inline on the caller path (blocking the actor until worker threads drained). start_handshake assumed cancel drained the old worker before staging a new session. Relay worker DNS resolution (to_socket_addrs) had no timeout or shutdown path. PoolRelayClient::shutdown did a synchronous handle.join() on the caller path.

## Trigger

D4 requirement: cancel() must never block the actor/caller. Codex review round 1 found relay-client shutdown still joined inline. Codex review round 2 found the detach introduced a stale-worker race: old worker can later call install_session/install_completed_signer and overwrite the new session, and DNS (to_socket_addrs) remains unbounded.

## Decision

cancel() is signal-only: sets cancel flag, drains pending signs, calls signal_shutdown (non-blocking, drops inbound_tx), spawns detached reaper that joins all worker handles off-path. Session-generation AtomicU64 guard: start_handshake/cancel/restore each bump the generation; install_session and install_completed_signer refuse to act unless the worker's stamp matches the active session's generation. DNS bounded via resolve_with_deadline (detached helper thread, TCP_CONNECT_TIMEOUT deadline). Documented residual: TLS/HTTP-upgrade per-syscall inactivity timeouts allow trickling-peer edge case (bounded, rare, never blocks caller).

## Consequences

- cancel() returns immediately regardless of worker state — actor never blocks
- Stale workers from a prior session cannot clobber the new active session (generation guard under active mutex)
- Superseded pre-install workers self-clean: signal_shutdown + reaper
- DNS resolution bounded by TCP_CONNECT_TIMEOUT (common stuck-DNS case eliminated)
- Trickling-TLS residual documented: one worker thread may outlive cancel by bounded time if peer trickles bytes; full interruptible-socket rewrite explicitly out of D4 scope
- Reaper-join proof test: spawn_reaper fires observer only after all .join() calls return — replacing join with drop makes the test fail

## Open Tail

- Full interruptible-socket / total-handshake-deadline rewrite for TLS/HTTP upgrade is a separate, harder problem

## Evidence

- transcript lines 3560-4010
