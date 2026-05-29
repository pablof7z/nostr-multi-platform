---
title: ADR-0027 — Unified ActionModule Executor Trait (Complete)
slug: adr-0027-action-module-status
summary: ADR-0027 unified ActionModule executor trait is fully implemented in master; the dual registration seam is deleted and execute() is the sole dispatch path.
tags:
  - adr
  - action-module
  - substrate
  - architecture
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
---

# ADR-0027 — Unified ActionModule Executor Trait (Complete)

> ADR-0027 unified ActionModule executor trait is fully implemented in master; the dual registration seam is deleted and execute() is the sole dispatch path.

## Status

ADR-0027 (unified `ActionModule` executor trait) is **fully implemented in master** as of 2026-05-29. The implementation landed piecemeal via PRs #227–#247. No follow-up branches are needed; the design doc's "Proposed" status is a documentation lag only. [^752b5-6]

## What Landed

- The `ActionModule` trait has a typed `execute()` return signature used by ~17 impls across all NIP crates.
- The dual registration seam is deleted: `register_action_executor` is gone; `execute` is now the sole dispatch path.
- `register_action_module` is also removed.
- The ADR-0027 design doc exists in master at `docs/design/adr/0027-unified-action-module-trait.md` and describes the completed architecture accurately, though the status field still reads "Proposed". [^752b5-7]

## Stale Branch Signals

Two local branches (`adr-0027-stage-1-trait-only`, `feat/adr-0027-execute-on-all-action-modules`) previously looked like unfinished ADR-0027 work. Both were 800+ commit orphans with no merge-base to current master, created in a divergent May-21 epoch. Their substance shipped piecemeal via the real PRs. Both branches were deleted after content-verification confirmed `identity.rs` and test files are byte-identical to master. Do not recreate these branches. [^752b5-8]

## See Also
- [[account-operations-c-abi-symbols|Account Operations Must Use Bespoke C-ABI Symbols — Not dispatch_action]] — related guide

