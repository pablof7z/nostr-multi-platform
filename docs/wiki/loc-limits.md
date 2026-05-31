---
title: File LOC Limits — 300 Soft, 500 Hard
slug: loc-limits
summary: Files must not exceed 300 LOC (soft limit) or 500 LOC (hard limit).
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-26
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:3afdf0df-923b-46cb-8fa6-acc61358bb75
  - session:7174d4d4-371b-4b8e-87a6-91024c2b4c2a
---

# File LOC Limits — 300 Soft, 500 Hard

## LOC Limits

No changed file may exceed a 500-line hard ceiling. Overgrown files are refactored into submodules to stay under the 500-LOC ceiling (e.g., engine.rs 842→452, settings.rs 316→234, feature_snapshot.rs 390→496 with 3 new submodules).

<!-- citations: [^3afdf-2] [^7174d-2] -->
## See Also

