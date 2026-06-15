---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: active
subjects:
  - bunker-broker-cancel
  - session-generation-guard
  - concurrency-model
supersedes:
  - 2026-06-15-3-bunkerbroker-cancel-becomes-signal-only-with
related_claims: []
source_lines:
  - 3830-3957
captured_at: 2026-06-15T18:08:06Z
---

# Episode: BunkerBroker cancel() becomes signal-only with session-generation guard

## Prior State

BunkerBroker::cancel() performed caller-path joins (blocking the actor until the relay worker drained). start_handshake assumed cancel() drained the old worker before staging the new session.

## Trigger

Detaching cancel() (making it signal-only via signal_shutdown + spawn_reaper) removed the old guarantee that the prior worker was drained. Codex review caught that a stale old worker could later call install_session()/install_completed_signer() and overwrite the newly-staged session — a real correctness regression the detach introduced, silently masked by the original inline-join.

## Decision

cancel() is signal-only (zero caller-path joins, reaper handles all joins off-path). A monotonic generation: AtomicU64 on BunkerBroker guards every install path: start_handshake/cancel/restore each bump the generation; install_session(generation, ..) returns false and no-ops when the worker's stamp doesn't match; install_completed_signer likewise refuses when superseded. Superseded workers self-clean via signal_shutdown + reaper.

## Consequences

- cancel() never blocks the caller (actor-availability guarantee)
- Stale workers cannot clobber a new session (generation guard atomic under active mutex)
- Bounded DNS: resolve_with_deadline runs getaddrinfo on a detached helper thread with TCP_CONNECT_TIMEOUT deadline
- Documented residual: trickling-TLS peer can keep one worker thread alive past cancel (rare, adversarial-relay only, never blocks caller)
- Reaper-join proof strengthened via test observer that fires only after all .join() calls return

## Open Tail

- Full interruptible-socket/total-deadline rewrite for TLS explicitly out of scope (documented bounded residual)

## Evidence

- transcript lines 3830-3957
