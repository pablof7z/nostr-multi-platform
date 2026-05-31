---
title: Relay Event Channel & Backpressure
slug: relay-event-channel
summary: The relay_tx channel for relay-event messages is unbounded, allowing events from relay workers to accumulate without limit during a flood.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-27
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:c0765978-d977-4400-8274-96df7682b126
  - session:156aa64b-42e1-4d3b-96ce-25b31fc06fec
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# Relay Event Channel & Backpressure

## Channel Capacity

The relay_tx channel for relay-event messages is currently unbounded, allowing events from relay workers to accumulate without limit during a flood. The relay transport lacks backpressure — if a relay fires events faster than the actor can process them, there is no mechanism to push back (e.g., send REQ CLOSE, pause reads, or signal the ingester to shed load). ADR-0029 defines bounded channels with shed-load backpressure as the fix for this unbounded mpsc problem, though implementation is deferred. Backpressure should be resolved via this bounded-channel plus relay-side pause logic approach, not by replacing tungstenite. Additionally, V-58 tracks that the reconnect worker backoff is blind to the relay close reason (closed.rs:27,149).

<!-- citations: [^09da8-4] [^c0765-4] [^156aa-9] [^cd2b6-21] -->
## See Also

