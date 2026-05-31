---
title: Relay Worker Outbound Buffer & Per-Worker Channel
slug: relay-worker-outbound-buffer
summary: Outbound frames are not drained from a shared queue by relays; the actor thread pushes each `OutboundMessage` directly into a per-relay-worker mpsc channel, and
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-05-27
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:7e56b660-13cc-42c9-915c-f8f97ef826d9
---

# Relay Worker Outbound Buffer & Per-Worker Channel

## Per-Relay Outbound Buffer

Outbound frames are not drained from a shared queue by relays; the actor thread pushes each `OutboundMessage` directly into a per-relay-worker mpsc channel, and each worker buffers frames in a `pending: VecDeque<String>` if the socket is still connecting. [^7e56b-4]


Each relay URL gets its own `relay_worker` OS thread with exponential backoff reconnect (3s→300s cap with jitter), and pending frames are preserved in the worker's buffer during the reconnect loop. [^7e56b-5]

REQ frames and event/profile claims are not crash-safe; they are replayed from `current_plan` on reconnect and parked in memory until relays connect, but lost if the process is killed. [^7e56b-6]
## See Also

