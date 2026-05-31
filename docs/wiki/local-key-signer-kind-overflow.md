---
title: LocalKeySigner Kind Overflow Coercion
slug: local-key-signer-kind-overflow
summary: "LocalKeySigner silently coerces kind overflow to u16::MAX."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-05-29
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# LocalKeySigner Kind Overflow Coercion

## Kind Overflow Behavior

LocalKeySigner returns a typed error for kind overflow instead of silently coercing overflowing kind values to u16::MAX.

<!-- citations: [^cd2b6-10] [^42908-10] -->
## See Also

