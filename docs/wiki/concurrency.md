---
title: Concurrency
slug: concurrency
topic: concurrency
summary: "Polling is forbidden at every layer of the stack: no sleep+check loops, no Timer.scheduledTimer querying state, no try_recv+sleep spin loops, no Task while !can"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:019ec57a-fb01-7081-80c8-d7107f302049
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Concurrency

## Polling Prohibition

Polling is forbidden at every layer of the stack: no sleep+check loops, no Timer.scheduledTimer querying state, no try_recv+sleep spin loops, no Task while !cancelled sleep+check tasks. Blocking primitives or event-driven patterns must be used instead: Rust channels block with recv/recv_timeout, iOS consumes ViewBatch snapshots pushed by the kernel, and background persistence piggy-backs on existing event ticks with wall-clock gates. Completion-by-polling of parked receivers also violates this no-polling doctrine (D8); completions should be delivered as messages into the actor mailbox.

<!-- citations: [^019ec-15] [^2e544-409] -->
