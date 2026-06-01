---
title: FFI Surface Freeze Gate & ADR Requirement
slug: ffi-surface-freeze-gate
summary: Any modification to the frozen C-ABI FFI surface requires an Architecture Decision Record (ADR)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-01
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
  - session:37035e20-9c1c-418f-88f1-68e464b51ec7
---

# FFI Surface Freeze Gate & ADR Requirement

## FFI Surface Change Control

Any modification to the frozen C-ABI RFI surface requires an Architecture Decision Record (ADR). The C-ABI surface freeze gate only tracks net-new symbol names, so ABI signature changes on existing functions (e.g., adding a parameter) do not violate it. Adding a `force` parameter to an existing C-ABI function like `nmp_app_claim_profile` is an acceptable breaking change because none of the C-ABI surface is stable yet, and the surface freeze gate tracks net-new symbol names (a signature change shows as net 0). For example, V-68 stage 2 (giving OpenAuthor/OpenThread a `kinds` param) necessitates an ADR because it alters the frozen C-ABI FFI surface, even though it does not introduce new symbols. The embed architecture complies with ADR-0025 by using the existing projection registry seam and minting zero new FFI symbols.

<!-- citations: [^42908-5] [^38935-4] [^37035-1] [^37035-6] -->
