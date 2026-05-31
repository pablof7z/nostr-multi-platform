---
title: Backlog Prioritization — Opus-Led Ranking, Sonnet Review Gate, Parallel Dispatch
slug: backlog-prioritization-workflow
summary: "The full workflow for converting BACKLOG.md items into landed PRs: Opus prioritization by severity labels, file-disjointness as tiebreaker only, Sonnet review-gating every PR, and sequential merge with worktree cleanup."
tags:
  - backlog
  - workflow
  - multi-agent
  - review
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-23
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:c6c4eedd-935c-4304-bff1-e041952f2def
---

# Backlog Prioritization — Opus-Led Ranking, Sonnet Review Gate, Parallel Dispatch

> The full workflow for converting BACKLOG.md items into landed PRs: Opus prioritization by severity labels, file-disjointness as tiebreaker only, Sonnet review-gating every PR, and sequential merge with worktree cleanup.

## Priority Order — Section 1 HIGH First

The backlog's own stated priority labels are the primary filter for selecting items to work. Items are ranked by the backlog's own priority order: Section 1 HIGH items take precedence over Section 4 test-coverage items. Parallelizability (file-disjointness) is a tiebreaker within the same priority tier only — it must never override the priority labels. Items in Section 1 marked HIGH (e.g., V-68 Stage 2 D0 kind:1/6 policy, V-87 D1 startup cluster, V-90 D8 actor-thread blocking, V-52 v1 DX) must be worked before items in Section 4 (test coverage) regardless of how easy the Section 4 items are to parallelize. The Feature Backlog section of docs/BACKLOG.md Section 4 lists items F-01 through F-07 ordered by V1 blocking priority, and Section 5 contains an explicit post-V1 deferral table.

<!-- citations: [^4edd4-106] [^95d02-6] [^c6c4e-4] -->
## Opus Prioritization Step

Before dispatching any implementation agents, an Opus agent must triage the backlog with the correct instruction: rank by the backlog's stated priority labels, Section 1 HIGH items first, parallel-safety only as a tiebreaker. The backlog's own agent entry point instructs: pick the topmost Section 4 item not already in Section 2. The Opus agent must also route around live peer agents by identifying which files are currently being modified by concurrent agent processes, so the prioritization excludes items that touch busy files. docs/BACKLOG.md is the single source of truth for planning, backlog, and violation tracking, superseding all scattered trackers. It includes a hard invariant fundamental rule with an explicit staged-fix corollary for multi-week work. docs/arch-review-queue.md has a deprecation banner redirecting readers to BACKLOG.md Sections 1 and 4.

<!-- citations: [^4edd4-107] [^95d02-7] [^c6c4e-3] -->
## What Went Wrong — Parallelizability Over Priority

In the session where 8 backlog items were dispatched, the Opus agent was given the wrong objective: "find 8 pairwise-disjoint items that run in parallel." This made parallelizability the primary filter, surfacing the easy, file-disjoint tail (V-103/104/105 tests, V-89, V-59) and skipping the actual top-priority HIGH violations (V-68 Stage 2, V-87, V-90, V-52) because those are harder or touch busy files. The work that landed was on real backlog items and the work was sound, but they were the wrong items — the backlog's own rule that Section 1 takes priority over Section 4 was overridden by convenience. [^4edd4-108]

## File-Disjointness for Parallel Dispatch

Once the Opus agent produces a prioritized list, the orchestrator verifies file-disjointness: no two items in the dispatch batch may touch the same file. Opus must verify this before dispatching. Items touching files currently owned by live peer agents (identified by live PIDs and their active worktrees) are excluded from the dispatch. This ensures parallel PRs can merge independently without conflicts. [^4edd4-109]

## Review-Gate — Sonnet Review Before Merge

Every PR opened by an implementation agent must be reviewed by a Sonnet agent before merging. The review-gate catches: tautological tests (seeds nothing, asserts only that a struct exists), zero-hacks violations (dead-import suppression, dead code paths, false comments), and structural hacks (self-asserting hooks where a closure clearing a map is asserted as the map being empty). In one wave, the review-gate caught 3 substantive issues out of 8 dispatch items — 2 tautological tests and 1 zero-hacks violation — that would otherwise have landed on master. Each was sent back for a genuine fix, re-reviewed, then merged. No work was lost: items that received BLOCK or CHANGES REQUESTED verdicts were immediately sent to fix agents for rework. [^4edd4-110]

## Review-Verdict Actions

APPROVE verdicts allow the PR to proceed to merge (once CI is green). BLOCK verdicts stop the PR and require the implementation agent to rework the item — the PR is sent back with the reviewer's precise fix list. CHANGES REQUESTED verdicts are similar to BLOCK but for smaller cleanups rather than structural problems. After rework, a fresh re-review is dispatched on the updated branch. This loop repeats until APPROVE is reached or the item is escalated to a different agent tier. [^4edd4-111]

## Session Recap and Merge Tracking

Each session that dispatches backlog items produces a final recap listing every PR merged (with number, description, and status), the final master commit SHA, and any escalated follow-ups that were not forced. The recap must distinguish between items that landed cleanly on first review and items that were blocked and reworked. Format is a table with Status, PR, Item, and Notes columns. [^4edd4-112]


## Completed and Partial Entry Maintenance

Completed backlog entries must be removed entirely (not merely marked DONE), and partial entries must be trimmed to keep only remaining open work. This prevents stale items from cluttering priority triage. [^4edd4-209]
## See Also
- [[sonnet-review-gate-workflow|Sonnet Review Gate — Mandatory PR Review Before Merge]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[backlog-citations-must-match-head|BACKLOG.md Violation Entries Must Cite File:Line Verified Against Current HEAD]] — related guide

