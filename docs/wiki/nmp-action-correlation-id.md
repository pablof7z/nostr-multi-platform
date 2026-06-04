---
title: NMP Action Correlation ID Threading
slug: nmp-action-correlation-id
summary: The NMP kernel threads the correlation ID through PublishUnsignedEventToRelays so that all action spinners close correctly.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-06-03
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:89070aba-0e77-4da3-99e1-322addb1c747
  - session:83b5dae5-d3f4-4f4d-b12f-9d04d17c9139
---

# NMP Action Correlation ID Threading

## Correlation ID Threading

The Core Action Registry validates JSON actions, mints correlation IDs, and dispatches `ActorCommand`s keyed by module namespace. The NMP kernel threads the correlation ID through PublishUnsignedEventToRelays so that all action spinners close correctly. The Swift UI stores the correlation ID returned by `model.zap(...)` in a `@State var pendingZapCid: String?`. The `signed_events` projection is drain-once: a correlation ID appears on exactly one snapshot frame before being cleared.

<!-- citations: [^2c4ad-9] [^54ae9-10] [^89070-1] [^83b5d-7] -->
