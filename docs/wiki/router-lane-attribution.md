---
title: Router Lane Attribution — Per-Lane RouteAttempt Observability (V-75)
slug: router-lane-attribution
summary: GenericOutboxRouter records per-lane RouteAttempt (RoutingLane, LaneOutcome) in PublishTrace/SubscriptionTrace, gated on tracing_active; AppRelayFallback is an explicit Lane 7 variant.
tags:
  - router
  - routing
  - v75
  - observability
  - tracing
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Router Lane Attribution — Per-Lane RouteAttempt Observability (V-75)

> GenericOutboxRouter records per-lane RouteAttempt (RoutingLane, LaneOutcome) in PublishTrace/SubscriptionTrace, gated on tracing_active; AppRelayFallback is an explicit Lane 7 variant.

## What V-75 Adds

The `GenericOutboxRouter` in `crates/nmp-router/src/router.rs` now records a per-lane `RouteAttempt` for every routing decision. This extends the V-51 observer seam (`PublishTrace` / `SubscriptionTrace`). [^42908-53]

## New Types

- `RoutingLane` — enum of all routing lanes including `AppRelayFallback` (the dedicated Lane 7 variant)
- `LaneOutcome` — `Matched { count }` or `Empty`; count = admissible-pass count, not net-new keys
- `RouteAttempt` — `{ lane: RoutingLane, outcome: LaneOutcome }`
- `PublishTrace.attempts: Vec<RouteAttempt>` and `SubscriptionTrace.attempts: Vec<RouteAttempt>`

Defined in `nmp-core/src/substrate/routing_trace.rs`. [^42908-54]

## Behavior Rules

- Lane 4/6 only emit attempts when applicable (not-applicable = absent from attempts list)
- `AppRelayFallback` is a dedicated `RoutingLane` variant to make the Lane 7 signal explicit
- Attempt tracking is gated on `tracing_active` (D8 compliance — no hot-path overhead)
- `explicit_targets` path produces empty attempts (does not go through lane logic)
- `lane_attempts` is serialized in `routing_trace_dto.rs` JSON output [^42908-55]

## See Also

