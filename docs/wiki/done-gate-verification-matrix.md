---
title: DONE Gate & Verification Matrix
slug: done-gate-verification-matrix
summary: The DONE gate requires every component to render correctly on the running app across all platforms with no hacks, no blank image placeholders, and no unresolved
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

# DONE Gate & Verification Matrix

## DONE Gate Verification Matrix

The DONE gate requires every component to render correctly on the running app across all platforms with no hacks, no blank image placeholders, and no unresolved hex pubkeys where names belong. The verification matrix has 64 cells (4 platforms × 16 components: 5 user, 1 relay, 6 content, 4 embed) each checked only after seeing it render correctly on the running app. The final deliverable is a PDF combining the verification matrix and every screenshot so the user can independently review whether the 'it works' claim is true. [^6a951-5]


## No-Hacks Rules

The no-hacks rules prohibit hex where names belong, 'Loading'/'Fetching' as a final state, blank image placeholders, shell pre-warming, hidden claim-trigger components, and the kernel fetching kind:0 off an event. [^6a951-6]

## Verification Protocol

Verification screenshots must be captured during a single warm session (no force-stopping between components) so that the kernel has time to resolve profiles and events via claims. Never trust an 'it works' claim without verifying it directly — always investigate broken observations rather than explaining them away as flakiness or timing. [^6a951-7]

## Code Push and Completion Rules

Never push code without verifying the whole thing compiles and all tests pass; never call anything 'done' off green CI — the final gate is the 64-cell matrix verified on running apps plus the review PDF. [^6a951-8]
## See Also

