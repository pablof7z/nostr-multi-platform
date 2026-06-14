---
title: Action Lifecycle
slug: action-lifecycle
topic: codebase-patterns
summary: "Action feedback collapses to a single mechanism: action_lifecycle with TTL-anchored retention, where ack serves as an early-dismiss only"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# Action Lifecycle

## Action Lifecycle

Action feedback collapses to a single mechanism: action_lifecycle with TTL-anchored retention, where ack serves as an early-dismiss only. The action_results drain and action_stages ack-mirror are deleted after host migration. A linear ActionTicket replaces the three correlation-id regimes so that a minted correlation id cannot fail to reach a terminal state. ActionTicket is a linear #[must_use] type whose Drop records Failed{dropped} via the actor channel, structurally eliminating the ~15-site spinner-hang bug class. correlation_id: Option<String> parameters are banned in nmp-core outside the ticket module.

<!-- citations: [^2e544-405] [^2e544-423] [^78c8e-479] [^2e544-458] -->
