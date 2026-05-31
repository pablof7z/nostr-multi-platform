---
title: No Feature Flag Deferrals
slug: no-feature-flag-deferrals
summary: Feature flags must not be added to defer decisions
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-23
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:1670fcb8-f275-498c-975b-8bd912331ded
---

# No Feature Flag Deferrals

## No Feature Flag Deferrals

Feature flags must not be added to defer decisions. When facing a tradeoff, name it explicitly and pick a side rather than hiding behind a flag. There are zero exceptions to clean architecture — all exceptions are eliminated completely by finishing the work properly, not by feature-gating or deleting incomplete features. There is zero tolerance on hacks — no temporary hacks, no fragmentation, no debt; everything by the book.

<!-- citations: [^1c093-30] [^1670f-17] -->
## See Also

