---
title: Sonnet Review Gate — Mandatory PR Review Before Merge
slug: sonnet-review-gate-workflow
summary: Every PR from a backlog-dispatch wave must be reviewed by a Sonnet agent before merging; BLOCK/CHANGES REQUESTED verdicts send PRs back for rework.
tags:
  - review
  - workflow
  - quality
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Sonnet Review Gate — Mandatory PR Review Before Merge

> Every PR from a backlog-dispatch wave must be reviewed by a Sonnet agent before merging; BLOCK/CHANGES REQUESTED verdicts send PRs back for rework.

## Dispatch Model

The workflow dispatches implementation agents (Haiku for mechanical items, Sonnet for structural items) in parallel to isolated worktrees, each owning a set of files verified to be disjoint from all other agents. Each agent opens a PR with scoped tests green. Every PR must be reviewed by a Sonnet agent before merging, scrutinizing the diff. No PR merges while CI (cargo test) is pending. The orchestrator gates all merges behind the Sonnet review verdict.

<!-- citations: [^4edd4-125] [^4edd4-232] -->
## Review Verdicts

APPROVE: The PR is clean and can merge once CI is green. The reviewer provides detailed code-traced evidence for the approval. BLOCK: The PR has a structural problem (tautological test that asserts nothing, self-asserting hook, or fundamentally wrong approach). The PR is sent back for a genuine rework with the reviewer's precise fix list. CHANGES REQUESTED: The PR has smaller violations (dead import suppressed with let _, false comments, dead code paths) that need cleanup. The core work is sound but needs polishing. After rework, the agent force-pushes to the same branch and a fresh re-review is dispatched. [^4edd4-126]

## Review Quality — Examples Caught

In the 8-item wave, the review-gate caught: (1) V-103 — a tautological D1 test that seeded nothing and asserted only that projections exists; a kernel with its store-read path severed would still pass. Sent back for rework into a genuine falsifiable test that seeds a real kind:1 event and asserts the exact content appears. (2) V-104 — a fake negentropy test that installed its own coverage hook doing plan.per_relay.clear() then asserted the plan was empty, proving only that a closure can clear a map. The original agent independently replaced it with the real T129 WatermarkFn-to-since mechanism. (3) V-105 — a zero-hacks violation: let _ = wait_barrier suppressing a dead-import warning plus a false comment claiming Barrier-then-snapshot when it was actually snapshot-drain, plus dead map-shape code. Sent back for 6 cleanups. [^4edd4-127]

## Review-Gate Effectiveness

The review-before-merge discipline is the highest-value part of the dispatch workflow. Of 8 backlog items dispatched in one wave: 6 reviews came back APPROVE with detailed code-traced evidence, 1 was BLOCKED and sent back for rework, and 1 had CHANGES REQUESTED for cleanup. All 3 issues were corrected with genuine fixes rather than waved through. This prevented 2 tautological tests and 1 zero-hacks violation from reaching master — issues that CI alone cannot detect. [^4edd4-128]

## See Also
- [[backlog-prioritization-workflow|Backlog Prioritization — Opus-Led Ranking, Sonnet Review Gate, Parallel Dispatch]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide
- [[collision-handling-two-agents-one-branch|Agent Collision Handling — Two Agents Targeting One Branch]] — related guide

