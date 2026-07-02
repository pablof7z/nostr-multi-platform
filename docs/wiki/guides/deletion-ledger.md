---
title: Architecture Deletion Ledger and Ratchets
slug: deletion-ledger
topic: deletion-ledger
summary: Each architecture slice carries a deletion ledger that tracks old doors deleted or privatized, new concepts introduced, and the net change in permanent concepts
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:019f0dc3-5b56-79d1-a14b-5746c93e5879
---

# Architecture Deletion Ledger and Ratchets

## Deletion Ledger

Each architecture slice carries a deletion ledger that tracks old doors deleted or privatized, new concepts introduced, and the net change in permanent concepts. Ratchets prevent old-pattern counts from rising once a slice has reduced them.

Each agent-ready slice is self-contained with Problem → Evidence (with real file paths on master) → Target state → Scope → Out-of-scope → Acceptance criteria → Verification commands.

Slices map 1:1 onto the ADR spine: READ slices → ADR-0070, WRITE slices → ADR-0071, starter slices → ADR-0069, and ratchet mechanics → ADR-0073's discipline.

The build→migrate→delete ordering is enforced: deletions follow landed replacements.

The simplification target is fewer public concepts, fewer app-facing lifecycle recipes, fewer read/write doors, fewer hidden defaults, fewer native-owned protocol decisions, and fewer duplicate sources of truth; not fewer files at any cost.

Every concept has exactly one canonical representation and one code path; if two paths exist for the same concern, one must be deleted before the PR merges.

The campaign estimate for net lines deletable across the whole ADR reset is approximately 12k–30k, concentrated in collapsing duplicate per-feature lifecycle wiring in nmp-core/runtimes.

<!-- citations: [^3c942-ac58b] [^3c942-e3b0b] [^3c942-89f70] [^019f0-e5a12] [^3c942-3570b] -->
