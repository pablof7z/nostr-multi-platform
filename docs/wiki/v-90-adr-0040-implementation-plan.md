---
title: V-90 Implementation Plan — 3 PR Sequence for ADR-0040
slug: v-90-adr-0040-implementation-plan
summary: "V-90 implementation is delivered as three independently-shippable PRs in sequence:  1"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# V-90 Implementation Plan — 3 PR Sequence for ADR-0040

## Implementation Plan

V-90 implementation is delivered as three independently-shippable PRs in sequence:

1. **Site 1** – DM `op.wait` off-actor.
2. **Site 3** – Cold-start signs via `PendingSign`.
3. **Site 2** – Capability-worker seam; the new primitive lands last under the fullest test coverage. [^4edd4-239]

## See Also

