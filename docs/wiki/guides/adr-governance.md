---
title: ADR Lifecycle and Governance
slug: adr-governance
topic: adr-governance
summary: ADR-0073 is the framing-rule ADR that establishes the 'not a museum' principle for the ADR directory and the ratchet/follow-up discipline for folded/amended ADR
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
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
---

# ADR Lifecycle and Governance

## ADR Directory Governance

ADR-0073 is the framing-rule ADR that establishes the 'not a museum' principle for the ADR directory and the ratchet/follow-up discipline for folded/amended ADRs. The active ADR directory is not a museum; it is the current decision surface. The directory uses a Current/Amended/Folded/Retired ledger. ADRs 0069–0073 are the live redesign spine; prior ADRs were folded or retired via PR #2324. ADR cleanup has failed if a new contributor reads the directory and comes away thinking production register_defaults(), app-facing open_interest, public ReducedSource, or hand-wired ObservedProjectionSink is the intended future. The directory should be collapsed into a small current set of approximately 4–6 records: app model and explicit composition; typed read sessions and output ownership; dynamic source reconciliation as private machinery; write flow with construction, signing, publishing, publish ledger, and route provenance; runtime, capability, and shell boundary; and possibly storage, replay, and sync invariants. Every existing ADR is classified as folded into a redesign ADR, folded into another durable owner, still-current standalone invariant, or retired/deleted. Old ADRs survive only for invariants that don't conflict with the spine; otherwise they are folded, amended, or retired in place, and git history is the archive. Every ADR 0001–0068 is tagged Current, Amended, Folded, or Retired with a named owner. A PR touching a folded or amended ADR's implementation area must keep old public-surface counts flat-or-decreasing, or update the owning ADR in place, producing no new correction docs that leave stale guidance behind. ADR-0030 (UniFFI/C-ABI two-surface split) is Current, amended by ADR-0072.

<!-- citations: [^3c942-be52a] [^019f0-1cf49] [^3c942-7f62b] [^898a4-fffe4] -->
