---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - bunker-broker-cancel-detach
  - session-generation-guard
  - stale-worker-race
supersedes:
  - 2026-06-15-4-bunkerbroker-cancel-must-be-signal-only
related_claims: []
source_lines:
  - 3660-3674
  - 3877-3900
  - 3904-3933
captured_at: 2026-06-15T16:55:55Z
---

# Episode: Detached BunkerBroker::cancel() requires generation guard against stale-worker race

## Prior State

BunkerBroker::cancel() joined inline on the caller path (handshake + dispatcher threads), blocking the actor until workers drained.

## Trigger

Codex review of initial detach found cancel() still called session.relay.shutdown() which does a synchronous handle.join() (relay_client.rs:289). Second review found that after detaching, start_handshake assumes cancel drained the old worker — a stale late install_session/install_completed_signer can overwrite the freshly-staged new session.

## Decision

cancel() is signal-only (zero caller-path joins): cancel flag + signal_shutdown + spawn_reaper for all joins. Monotonic AtomicU64 generation guard added: start_handshake/cancel bump generation, stamp it on ActiveSession and worker; install_session and install_completed_signer refuse to install when worker generation doesn't match active generation. Superseded workers self-teardown via signal_shutdown + reaper off-path.

## Consequences

- Stale-worker-can-clobber-new-session race eliminated (non-vacuous test: removing generation check fails the test)
- DNS resolution bounded: resolve_with_deadline runs getaddrinfo on helper thread, worker waits only TCP_CONNECT_TIMEOUT
- Documented residual: trickling-TLS peer can keep one worker thread alive past cancel (per-syscall inactivity timeout, not total deadline); rare, adversarial, leaks at most one thread, never blocks caller
- Reaper-join proof: reaper_observer fires only after all .join() calls return; test verifies observer has NOT fired while threads are parked (non-vacuous: replacing join with drop fails)

## Open Tail

- Full interruptible-socket/total-deadline rewrite explicitly out of D4 scope — trickling-TLS residual accepted
- D4 codex round-3 review in flight

## Evidence

- transcript lines 3660-3674
- transcript lines 3877-3900
- transcript lines 3904-3933
