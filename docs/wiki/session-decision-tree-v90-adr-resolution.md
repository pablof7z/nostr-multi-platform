---
title: Session Decision Tree — V-90 ADR Resolution and User Choices
slug: session-decision-tree-v90-adr-resolution
summary: "The user decision process at the end of the backlog wave: chose option 1 (draft V-90 ADR), then option 3 (merge as Proposed, defer implementation), moving V-90 into a ratifiable state."
tags:
  - decision-tree
  - v-90
  - adr-0040
  - backlog
  - session
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Session Decision Tree — V-90 ADR Resolution and User Choices

> The user decision process at the end of the backlog wave: chose option 1 (draft V-90 ADR), then option 3 (merge as Proposed, defer implementation), moving V-90 into a ratifiable state.

## The Decision Point

After the correctly-prioritized backlog wave (V-52, V-42, V-87, V-68-S2, V-60) landed at master bb8bc105, the parallel-safe ungated HIGH set was exhausted. The remaining HIGH items were all genuinely blocked: V-90 needed an ADR, V-51-p3 was pure UI contended by live chirp peers, and V-68-S2 author-half + V-87 iOS legs were behind the live profile-fetch peer. The user was presented with four options: (1) draft the V-90 ADR, (2) iOS legs of V-87/V-68-author after peer quiesces, (3) F-02/F-04 verification harness (live-relay, live-NWC), (4) continue down MEDIUM Section-1 items. [^4edd4-185]

## User Choice — Option 1: Draft V-90 ADR

The user chose option 1: draft the V-90 ADR (capability-worker seam). This is the highest-priority blocked HIGH item, and the blocker is a design decision — so the right deliverable is a properly-grounded ADR for ratification, not code. An Opus architect was dispatched to draft ADR-0040 as a ratifiable ADR PR with an executive summary, following the existing ADR convention (next after ADR-0039). [^4edd4-186]

## ADR-0040 Presented — Three Options

ADR-0040 was drafted as PR #842 with a three-primitive design (PendingSign reuse, lnurl worker reuse, single capability-worker thread). The user was given three ratification options: (1) Ratify — flip Status to Accepted, merge, and start implementation; (2) Request changes; (3) Merge as Proposed, defer implementation — land the ADR for the record, implement later. [^4edd4-187]

## User Choice — Option 3: Merge as Proposed

The user chose option 3: merge ADR-0040 as Proposed for the record, defer implementation. Before merging, the ADR was fact-checked: off-by-one citations in identity.rs (826,864,1019 → 825,863,1018) were corrected, and a lnurl re-entry clarification was added. The ADR was merged as Proposed, moving V-90 from 'ADR-gated with no actionable path' to 'design ratifiable, implementation plan ready' with three independently-shippable implementation PRs deferred until explicitly greenlit. [^4edd4-188]

## Honest Blocker Map at Decision Point

At the decision point, the remaining blockers were: V-90 — ADR-gated (now resolved by ADR-0040), V-51-p3 — pure UI contended by live chirp-tui/desktop peers, V-68-S2 author-half + V-87 iOS legs — behind live profile-fetch peer, F-01 — out of v1 (wasm deferred post-v1), F-02/F-04 — verification tasks needing a live-relay/live-NWC harness that doesn't exist yet (though the user provided nak serve and relay.primal.net as tooling). The orchestrator must not auto-descend into the MEDIUM tail when HIGH items are blocked — it must surface the decision point to the user. [^4edd4-189]

## See Also
- [[v-90-adr-0040-capability-worker-seam|V-90 — ADR-0040 Capability-Worker Seam (HIGH · D8, ADR-Gated)]] — related guide
- [[adr-0040-capability-worker-seam-full-design|ADR-0040 — Capability-Worker Seam Full Design and Ratification]] — related guide

