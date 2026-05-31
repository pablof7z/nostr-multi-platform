---
title: M10.5 FFI Proof Gate — Hard Gate Before M11
slug: m10-5-ffi-proof-gate
summary: M10.5 is a hard gate that must pass empirical FFI proof before any work proceeds to M11.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:7f0f0c78-d1aa-49db-b659-c9cf49827117
  - session:e50d12a1-0a49-4a45-9bb1-251fa0f434b6
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
---

# M10.5 FFI Proof Gate — Hard Gate Before M11

## FFI Proof Gate

M10.5 is a hard gate that must pass empirical FFI proof before any work proceeds to M11. M10.5's original literal exit gate was over-specified for non-existent iPhone-12 hardware and an M1–M10 iOS slice; hardware-dependent items are DEFERRED to the Pulse track, and only the simulator-provable subset is finalized. M10.5 closes on the achievable simulator-provable subset with the S2 working-set overrun explicitly open and routed to the kernel session, not waived or threshold-revised. The required exit-gate artefacts (sim-baseline.md, doctrine-review.md, Instruments evidence, and docs/perf/m10.5/) are currently absent.

<!-- citations: [^7f0f0-7] [^e50d1-3] [^57528-13] -->
## Scope

The FFI-hardening session is scoped to docs/ffi-surface.md, docs/perf/m10.5/**, docs/design/ffi-hardening.md, ios/NmpStress/**, and crates/nmp-testing/bin/ffi-stress/**, and must NOT touch crates/nmp-core/**, docs/builder-guide/**, or Cargo.*. [^e50d1-4]

## FFI Surface Inventory

docs/ffi-surface.md must enumerate every exported C symbol in crates/nmp-core/src/ffi.rs, every type crossing the boundary, every capability trait, and the ownership/lifetime invariant (who allocates, who frees, thread-affinity, nullability) for each, citing ffi.rs:line and tagging it reviewed <date>. [^e50d1-5]

## Document Standards

Files must stay ≤300 soft / ≤500 hard LOC, with no TODO/FIXME/placeholder prose and no faked numbers; if a result cannot be obtained on the simulator, it must be explicitly stated and deferred to the Pulse track. [^e50d1-6]

## Out of Scope

The bounded channel fix must use try-send + drop/coalesce, must stay D6 fire-and-forget (never block FFI, never error across the boundary), and belongs to the kernel session's scope, not the FFI-hardening workstream. [^e50d1-7]
## See Also

