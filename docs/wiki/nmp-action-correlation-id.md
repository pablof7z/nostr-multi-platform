---
title: NMP Action Correlation ID Threading
slug: nmp-action-correlation-id
summary: The NMP kernel threads the correlation ID through PublishUnsignedEventToRelays so that all action spinners close correctly.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-05-28
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
---

# NMP Action Correlation ID Threading

## Correlation ID Threading

The NMP kernel threads the correlation ID through PublishUnsignedEventToRelays so that all action spinners close correctly. [^2c4ad-9]


The Core Action Registry validates JSON actions, mints correlation IDs, and dispatches `ActorCommand`s keyed by module namespace. The NMP kernel threads the correlation ID through PublishUnsignedEventToRelays so that all action spinners close correctly. [^54ae9-10]
## See Also

