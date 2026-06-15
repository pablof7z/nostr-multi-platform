---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - bunker-broker-cancel
  - d4-detach
  - signer-concurrency
supersedes: []
related_claims: []
source_lines:
  - 3561-3591
captured_at: 2026-06-15T15:36:07Z
---

# Episode: BunkerBroker cancel() made signal-only with detached reaper thread

## Prior State

BunkerBroker::cancel() joined the handshake and dispatcher threads inline on the caller's path (handle.join() at broker.rs:177,187), blocking the caller until both threads fully exit. The test cancel_joins_inbound_dispatcher_thread_no_leak enshrined this join-on-path contract.

## Trigger

D4 audit item identified that inline joins block the capability/actor call path; D4 was flagged as a real PR needing completion.

## Decision

cancel() now performs signal-only teardown (set cancel flag → drain pending → relay.shutdown → spawn_reaper) and returns immediately. The detached background reaper thread (nmp-broker-cancel-reaper) joins both handles off the call path. install_completed_signer's race-guard also routes orphaned dispatchers through spawn_reaper for uniformity.

## Consequences

- Caller (capability/actor thread) is never blocked by thread wind-down — cancel returns in <1s even if worker is still running
- Old test replaced with cancel_is_signal_only_does_not_block_on_join — regression-proven (restoring old inline-join behavior causes the new test to hang)
- No leak: reaper owns and joins both handles; threads self-exit reliably on existing signals (channel close, cancel flag, WorkerCmd::Shutdown)
- No D8/D9 violations — dispatcher blocks on recv (woken by channel close), reaper blocks on join

## Open Tail

*(none)*

## Evidence

- transcript lines 3561-3591
