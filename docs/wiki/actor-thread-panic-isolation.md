---
title: Host Callback and Actor Thread Panic Isolation
slug: actor-thread-panic-isolation
summary: "Host callback panic isolation must cover `ActionRegistry::deliver_result`, `event_observer.rs`, and `raw_event_observer.rs`, not just `Box<dyn Fn>` closures."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-26
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:200932fb-5a92-44e0-8d42-2184d2e69094
---

# Host Callback and Actor Thread Panic Isolation

## Panic Isolation Scope

Host callback panic isolation must cover `ActionRegistry::deliver_result`, `event_observer.rs`, and `raw_event_observer.rs`, not just `Box<dyn Fn>` closures. The supervisor clones `update_tx_panic` before spawning so that panic frames still reach the listener after the actor's own sender is dropped.

<!-- citations: [^1c093-6] [^20093-1] -->
## See Also

