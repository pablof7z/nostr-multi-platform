---
title: Product Correction Escalation
slug: product-correction-escalation
topic: developer-workflow
summary: When a user gives a correction or instruction about how the NMP product should work, it must be treated as a possible product-authority update, not just an impl
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-18
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
---

# Product Correction Escalation

## Product Correction Escalation

When a user gives a correction or instruction about how the NMP product should work, it must be treated as a possible product-authority update, not just an implementation request. Before making code changes for the correction, a separate agent must research whether the correction should be represented in product specs, doctrine, canonical docs, docs/plan.md, GitHub Issues, or ADRs under docs/decisions/. If documentation needs to change for a product correction, the documentation update must be made in the same PR as the implementation unless the user explicitly scopes the work to docs only.

<!-- citations: [^019ed-130] [^019ed-134] -->
