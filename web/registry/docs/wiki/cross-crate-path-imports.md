---
title: Cross-Crate Path Imports
slug: cross-crate-path-imports
summary: "Cross-crate `#[path]`-shared registry modules must use `super::` relative imports (e.g"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Cross-Crate Path Imports

## Cross-Crate `#[path]` Shared Modules

Cross-crate `#[path]`-shared registry modules must use `super::` relative imports (e.g. `super::super::nostr_mention_chip`), not `crate::` absolute imports, because the same file is compiled under different crate roots with different module layouts. [^6a951-7]

## See Also

