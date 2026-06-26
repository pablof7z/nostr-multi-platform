# ADR-0040 — Capability worker seam

- **Status:** Accepted / implemented
- **Date:** 2026-05-31
- **Relates to:** ADR-0024, ADR-0028, ADR-0031

## Context

The kernel actor is the single writer of kernel state. Blocking that actor on
remote-signer work or platform capability I/O freezes command handling, relay
processing, and snapshot emission.

Some capability classes are synchronous on the platform side, especially
Keychain-style persistence. The platform callback can remain synchronous, but it
must not run on the actor thread for liveness-sensitive paths.

## Decision

NMP uses off-actor workers for blocking signer/capability work:

1. NIP-17 gift-wrap work that waits on remote signer operations runs off actor
   and re-enters with typed actor commands.
2. Native capability work uses one long-lived serialized capability worker
   thread owned by the app handle.

The capability worker drains a FIFO queue with blocking `recv`, executes the
existing platform capability callback on the worker thread, and re-enters the
actor with `ActorCommand::CapabilityResultReady`.

## Account Correctness

Each work item carries the originating account id and correlation id. The actor
checks the account on re-entry and drops stale results for removed accounts.

The single FIFO worker preserves persist/forget ordering for the same account.
That ordering matters more than parallel throughput because capability writes are
low-frequency and can change durable secret state.

## Liveness

Worker code must not poll. A wedged operation reports a timeout/error result as
data and does not mutate kernel state from the worker thread.

Cold-start local Keychain reads that happen before the first snapshot may remain
synchronous when they are known not to involve biometric/UI wait paths. Blocking
write paths use the worker.

## Consequences

- The actor does not block on multi-second signer/capability waits.
- Platform capability callbacks remain simple synchronous functions.
- Durable account persistence keeps FIFO ordering.
- Actor liveness remains observable through the normal update stream and
  liveness probes.
