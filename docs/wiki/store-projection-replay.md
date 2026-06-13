---
title: Store-Projection Replay
slug: store-projection-replay
topic: store-projection-replay
summary: ADR-0045 storeâprojection replay is accepted (staged); implementation is tracked separately
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Store-Projection Replay

## Store→Projection Replay

ADR-0045 store→projection replay is accepted (staged); implementation is tracked separately. The resubscribe fix also gets store-replay for free via the #1237 push_interest_and_serve choke point, replaying stored-but-unprocessed kind:445s on restart.

The feedback one-change fix collapses to one mechanism with one retention policy: `action_lifecycle` is the sole host-facing projection with TTL-anchored retention where ack is purely early-dismiss.

iOS `SignerStateTone.derivedLabel` and `WalletStatusTone.derivedLabel` duplicate Rust business logic by switching on raw state tokens as `??` fallbacks for 'older buffers' that cannot exist in a running app (once a build ships the fields, old buffers from the same process are impossible).

<!-- citations: [^02745-19] [^da6b1-39] [^78c8e-31] [^2e544-70] -->
