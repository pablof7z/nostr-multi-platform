# ADR-0029 — Actor Queue Observability And Backpressure Policy

- **Status:** Accepted
- **Date:** 2026-05-22

## Decision

The live actor command channel is observable through queue-depth diagnostics and
is paired with bounded work at the producers that can overload it.

Backpressure belongs at typed workload boundaries:

- capability and worker queues use bounded execution,
- publish and signing flows report durable action state,
- scheduler-owned wakeups are explicit,
- queue depth is diagnostic data, not hidden policy.

Any future actor-channel shedding policy must be justified by measured
queue-depth data and must preserve command ordering and correctness.

## Consequences

- Do not add sleep/poll loops to manage actor pressure.
- Do not drop user intents silently.
- Prefer bounded workers, typed action state, and observable queue metrics over
  implicit channel behavior.
