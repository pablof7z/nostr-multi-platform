---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - d4-cancel-detach
  - bunker-broker
  - reaper-thread
supersedes:
  - 2026-06-15-4-bunkerbroker-cancel-made-signal-only-with
related_claims: []
source_lines:
  - 3561-3592
  - 3665-3678
  - 3836-3879
captured_at: 2026-06-15T16:41:53Z
---

# Episode: BunkerBroker cancel must be signal-only with detached reaper

## Prior State

BunkerBroker::cancel() joined inline on the caller path (handshake thread handle.join() + dispatcher thread handle.join()), blocking the actor. PoolRelayClient::shutdown also did synchronous handle.join(). Cancel was not truly signal-only.

## Trigger

D4 plan item; first codex review found relay-client shutdown still joined inline (broker.rs:182 → relay_client.rs:289-302); second codex review found replacement-session race where old worker can call install_session/install_completed_signer and overwrite the new active session.

## Decision

cancel() performs signal-only teardown and returns immediately: set cancel flag (Acquire), drain pending signs, call signal_shutdown() (non-blocking channel drop), spawn detached reaper thread that joins all handles off-path. All joins uniformly in reaper, zero on caller path.

## Consequences

- First implementation missed that relay.shutdown() also joined inline — rework moved relay-client join into reaper too
- Replacement-session race identified: old worker can overwrite new session after detached cancel — needs session generation/token check before installing
- DNS resolution (to_socket_addrs) and TLS/HTTP upgrade still not shutdown-interruptible or absolute-deadline bounded
- Test rewritten from join-enshrining to signal-only contract (cancel returns in <1s while thread held on barrier)

## Open Tail

- Session generation/token check needed to prevent old worker overwriting new session after detached cancel
- DNS/TLS connect-path bounding still incomplete

## Evidence

- transcript lines 3561-3592
- transcript lines 3665-3678
- transcript lines 3836-3879
