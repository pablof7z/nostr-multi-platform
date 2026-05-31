---
title: Backlog Course Correction — Parallelizability Mistake and User Correction
slug: backlog-course-correction-parallelizability-mistake
summary: The first backlog wave incorrectly optimized for parallelizability over priority; the user's correction demanded Opus re-prioritize by the backlog's own severity labels, Section 1 HIGH first.
tags:
  - backlog
  - workflow
  - prioritization
  - course-correction
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Backlog Course Correction — Parallelizability Mistake and User Correction

> The first backlog wave incorrectly optimized for parallelizability over priority; the user's correction demanded Opus re-prioritize by the backlog's own severity labels, Section 1 HIGH first.

## The Mistake — Parallelizability Over Priority

In the first backlog dispatch wave, the Opus agent was given the wrong objective: 'find 8 pairwise-disjoint items that run in parallel.' This made parallelizability the primary filter. The Opus agent surfaced the easy, file-disjoint tail — V-103/104/105 (test coverage), V-89 (sentinel removal), V-59 (EventStore clock) — and skipped the actual top-priority HIGH violations: V-68 Stage 2 (D0 kind:1/6 policy), V-87 (D1 startup cluster), V-90 (D8 actor-thread blocking), and V-52 (v1 DX). The backlog's own rule — Section 1 takes priority over Section 4, pick the topmost item — was overridden by convenience. The 8 items that landed were real backlog items and the work was sound, but they were the wrong items. [^4edd4-136]

## User Correction

The user explicitly demanded that Opus prioritize the actual BACKLOG.md file by its stated severity labels: 'the backlog is in docs/BACKLOG.md! how can you claim that has landed?! did you even prioritize THAT backlog via the opus agent as I fucking demanded?' The orchestrator acknowledged the mistake: 'I optimized the prioritization for the wrong thing' and 'I did run the Opus agent on docs/BACKLOG.md. But I told it to find 8 pairwise-disjoint items. That framing made parallelizability the primary filter.' [^4edd4-137]

## The Correct Re-Prioritization

The corrected Opus instruction is: rank by the backlog's stated priority labels, Section 1 HIGH items first, parallel-safety only as a tiebreaker. The Opus agent must also route around live peer agents by identifying which files are currently being modified by concurrent agent processes. The re-prioritization produced a list with four parallel-safe Section-1 HIGH items leading, correctly deferring V-90 (ADR-gated), V-51-phase3 (contended), and the profile.rs/iOS legs (live peer collision). Before dispatching, the orchestrator verified file-disjointness — V-87 and V-68-S2 both reach into actor/mod.rs and cannot run in parallel, so V-68-S2 was sequenced behind V-87. [^4edd4-138]

## The Correctly-Prioritized Wave

Three parallel-safe HIGH items dispatched first: V-52 (single-relay browsing), V-42 (NIP-51 mute list), V-87 (D1 startup kernel half). V-68-S2 (thread half) was sequenced behind V-87 due to shared actor/mod.rs. After these landed, V-60 (MEDIUM · LMDB LRU eviction) was dispatched as the topmost unblocked Section-1 item. The remaining HIGH items are correctly blocked: V-90 needs an ADR ratified, V-51-p3 is pure UI contended by live chirp peers, and V-68-S2 author-half + V-87 iOS legs are behind the live profile-fetch peer. [^4edd4-139]

## Honest Blocker Map

When the parallel-safe ungated HIGH set is exhausted, the remaining HIGH items are genuinely blocked: V-90 needs an ADR ratified before any code (capability-worker seam), V-51-phase3 is pure UI heavily contended by live chirp-tui/desktop peers, and V-68-S2 author-half + V-87 iOS legs are behind the live profile-fetch peer agent. F-01 (IndexedDB) is out of v1 — wasm is not a v1 platform target. F-02 (DM cold-start) and F-04 (Zap E2E) are verification tasks that need a live-relay/live-NWC harness that doesn't exist yet. The orchestrator must surface these blockers rather than auto-descending into lower-priority tail work. [^4edd4-140]

## See Also

