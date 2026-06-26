# ADR-0024 — Non-blocking capability execution

- **Status:** Accepted / implemented
- **Date:** 2026-05-21
- **Relates to:** ADR-0023, ADR-0040

## Context

The actor must remain the single writer of kernel state and must not block on
multi-second or platform-owned I/O. Capability work that can block must run away
from the actor thread and re-enter as typed actor data.

## Decision

Long-running capability work uses an off-actor worker path:

- the actor enqueues a capability work item carrying a correlation id and the
  relevant account id;
- the worker drains its queue with blocking receives;
- the worker executes the platform capability callback off the actor thread;
- the result re-enters the actor as `ActorCommand::CapabilityResultReady`;
- the actor applies or drops the result on its own thread after checking the
  account still exists.

Microsecond-class synchronous capabilities may still resolve inline when they are
not running on a liveness-sensitive actor path.

## Requirements

- Workers do not mutate kernel state directly.
- Worker queues do not poll.
- Results carry enough identity/correlation data to avoid account-switch
  misapplication.
- Errors are data and re-enter as capability/action failures.

## Consequences

- HTTP-class and Keychain-class blocking work no longer freezes actor progress.
- Ordering-sensitive capability calls use the serialized worker from ADR-0040.
- The actor remains the only authority that mutates durable kernel state.
