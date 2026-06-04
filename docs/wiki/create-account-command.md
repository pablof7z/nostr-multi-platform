---
title: CreateAccount Command & Initial Event Publishing
slug: create-account-command
summary: "`CreateAccount` is the sole command that publishes kind:0 and kind:10002 events"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:d8869714-0ee5-4fe3-94db-1efd068b1c58
---

# CreateAccount Command & Initial Event Publishing

## CreateAccount Command

`CreateAccount` is the sole command that publishes kind:0 and kind:10002 events. It generates a keypair, publishes kind:0/10002, and calls `AddSigner(make_active: true)`. [^d8869-21]

## See Also

