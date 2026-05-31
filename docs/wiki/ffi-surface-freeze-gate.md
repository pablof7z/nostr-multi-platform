---
title: FFI Surface Freeze Gate & ADR Requirement
slug: ffi-surface-freeze-gate
summary: Any modification to the frozen C-ABI FFI surface requires an Architecture Decision Record (ADR)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# FFI Surface Freeze Gate & ADR Requirement

## FFI Surface Change Control

Any modification to the frozen C-ABI FFI surface requires an Architecture Decision Record (ADR). For example, V-68 stage 2 (giving OpenAuthor/OpenThread a `kinds` param) necessitates an ADR because it alters the frozen C-ABI FFI surface. The embed architecture complies with ADR-0025 by using the existing projection registry seam and minting zero new FFI symbols. [^42908-5]

<!-- citations: [^42908-5] [^38935-4] -->
## See Also

