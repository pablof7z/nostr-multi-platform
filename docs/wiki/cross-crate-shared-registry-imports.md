---
title: Cross-Crate Shared Registry Imports
slug: cross-crate-shared-registry-imports
summary: "Cross-crate `#[path]`-shared registry modules must use `super::` relative imports (e.g"
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

# Cross-Crate Shared Registry Imports

## Import Paths for #[path]-Shared Registry Modules

Cross-crate `#[path]`-shared registry modules must use `super::` relative imports (e.g. `super::super::nostr_mention_chip`) rather than `crate::` absolute paths because the shared file is compiled in multiple crates with different crate roots. [^6a951-3]

## See Also

