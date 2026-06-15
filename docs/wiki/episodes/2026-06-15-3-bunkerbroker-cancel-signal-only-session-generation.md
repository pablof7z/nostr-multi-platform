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
supersedes:
  - 2026-06-15-3-detached-bunkerbroker-cancel-requires-generation-guard
related_claims: []
source_lines:
  - 3561-4010
captured_at: 2026-06-15T17:04:02Z
---

# Episode: BunkerBroker cancel signal-only + session-generation guard prevents caller freeze and stale-worker clobber

## Prior State

BunkerBroker::cancel() joined inline on the caller/actor path (handle.join for handshake and dispatcher threads, plus relay.shutdown's synchronous join), blocking the actor until workers fully exited. The inline join implicitly guaranteed the old worker was drained before a new session staged.

## Trigger

D4 spec required signal-only cancel. Codex review round 1 found relay.shutdown still did a synchronous handle.join. Codex review round 2 found that detaching the join introduced a stale-worker race: with cancel now detached, the old worker can later call install_session/install_completed_signer and overwrite the newly-staged session.

## Decision

cancel() does zero caller-path joins: it signals (cancel flag + signal_shutdown dropping inbound_tx + relay WorkerCmd::Shutdown) and spawns a detached reaper thread that joins all handles off-path. A monotonic AtomicU64 session-generation guard stamps each session; install_session and install_completed_signer return false and no-op unless the worker's generation matches the active session's. Superseded workers self-clean by signal_shutdown + reaping their own relay. DNS bounded via resolve_with_deadline (detached helper thread, TCP_CONNECT_TIMEOUT ceiling).

## Consequences

- Actor/caller never blocks on cancel — the core D4 guarantee
- Stale workers cannot clobber a new session — generation guard check+swap is atomic under the active mutex
- DNS is bounded: common stuck-getaddrinfo case now fails within TCP_CONNECT_TIMEOUT
- Documented residual: TLS/HTTP upgrade uses per-syscall inactivity timeouts, not a total-handshake deadline; a maliciously trickling relay can keep one worker alive past cancel — explicitly out of D4 scope as it requires async-socket rewrite
- Test rewritten: cancel_is_signal_only_does_not_block_on_join proves cancel returns while workers are still alive; reaper_observer proves reaper genuinely joins handles (non-vacuous: drop-instead-of-join fails the test)
- Session generation bump also invalidates on cancel-without-restart

## Open Tail

- Full interruptible-socket / total-deadline TLS handshake is a separate scope beyond D4

## Evidence

- transcript lines 3561-4010
