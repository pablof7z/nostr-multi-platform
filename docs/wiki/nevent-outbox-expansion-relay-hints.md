---
title: Nevent Outbox Expansion & Relay Hints
slug: nevent-outbox-expansion-relay-hints
summary: For event-id (nevent) claims, outbox expansion cannot be used unless the bech32 includes the pubkey; the relay hint is followed first.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Nevent Outbox Expansion & Relay Hints

## Outbox Expansion for nevent Claims

For event-id (nevent) claims, outbox expansion cannot be used unless the bech32 includes the pubkey; the relay hint is followed first. [^6a951-10]


The time-shift trap must be avoided: when verifying a relay serves an event, the check must occur at the same instant as the claim, not at a different time when relay state may have changed. [^6a951-11]
## See Also

