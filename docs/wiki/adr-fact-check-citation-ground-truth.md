---
title: ADR Fact-Check — Citation Ground-Truthing Before Merge
slug: adr-fact-check-citation-ground-truth
summary: ADR citations must be ground-truthed against HEAD before merging — even Proposed-status ADRs. ADR-0040 had off-by-one identity.rs citations corrected during review.
tags:
  - adr
  - fact-check
  - citations
  - review
  - adr-0040
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# ADR Fact-Check — Citation Ground-Truthing Before Merge

> ADR citations must be ground-truthed against HEAD before merging — even Proposed-status ADRs. ADR-0040 had off-by-one identity.rs citations corrected during review.

## Overview

Even a Proposed (non-Accepted) ADR must have accurate file:line citations before merge, because it serves as the durable design record that future implementers will reference. A fact-check reviewer verified ADR-0040's citations against the live HEAD tree and found off-by-one errors that were corrected before the merge. [^4edd4-190]

## Findings — Mostly Accurate

The reviewer found ADR-0040 MOSTLY ACCURATE: the architecture, both reuse claims (PendingSign path, nmp-nip57 lnurl worker), account-switch reasoning, and ADR-0024 supersession were all sound. However, the identity.rs citations were off by one: 826,864,1019 should be 825,863,1018 — they pointed to the comment line or the next line after sign_active, rather than the actual sign_active call itself. This affected four places in the document. [^4edd4-191]

## Corrections Applied

All four off-by-one citations were corrected (826,864,1019 → 825,863,1018). Additionally, a lnurl re-entry clarification was added (Fix 3) so a future implementer isn't misled about how the nmp-nip57 worker re-enters the actor. The corrections were committed, pushed, and CI re-triggered before merging. [^4edd4-192]

## Procedure Rule

Every ADR, including Proposed-status ones, must have its file:line citations ground-truthed against the live HEAD tree before merging. A fact-check reviewer (Sonnet) verifies: citation accuracy (do line numbers point to the correct calls?), reuse-pattern claims (do the referenced patterns actually exist and match the description?), and architectural correctness. Off-by-one errors must be corrected before merge — a Proposed ADR with wrong citations sends future implementers to the wrong code locations. [^4edd4-193]

## See Also
- [[adr-0040-capability-worker-seam-full-design|ADR-0040 — Capability-Worker Seam Full Design and Ratification]] — related guide

