---
title: NIP-77 Negentropy Resume Full-Reseed
slug: nip-77-negentropy-resume-reseed
summary: NIP-77 resume always performs full-reseeds because `negentropy 0.5` exposes no public deserializer.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
---

# NIP-77 Negentropy Resume Full-Reseed

## Full-Reseed Behavior

NIP-77 resume always performs full-reseeds because `negentropy 0.5` exposes no public deserializer. [^57528-15]


## Canonicalise Closure Divergence Risk

The NIP-77 canonicalise closure is caller-supplied at `planner_gate.rs:70`, creating a divergence risk. [^57528-16]

## Status Field Initialization

The `nip77_negentropy` status field at `kernel/types.rs:141` must be updated from its permanent "unknown" state once the hook is installed. [^57528-17]
## See Also

