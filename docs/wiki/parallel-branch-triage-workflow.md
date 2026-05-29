---
title: Parallel Branch Triage — Haiku-Agent Fan-Out Workflow
slug: parallel-branch-triage-workflow
summary: Triage large sets of branches by dispatching one Haiku agent per branch in parallel; orchestrator collects verdicts, spot-checks not-on-origin candidates, then performs deletions.
tags:
  - git
  - branches
  - triage
  - haiku
  - agents
  - workflow
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
---

# Parallel Branch Triage — Haiku-Agent Fan-Out Workflow

> Triage large sets of branches by dispatching one Haiku agent per branch in parallel; orchestrator collects verdicts, spot-checks not-on-origin candidates, then performs deletions.

## Overview

When triaging a large set of branches (50+) for potential deletion, dispatch one Haiku agent per branch in parallel. Each agent runs read-only git investigation and returns a structured per-branch verdict. The orchestrator collects all verdicts, then performs deletions itself (agents never delete). [^752b5-13]

## Triage Workflow

1. Pre-compute cheap shared facts for all branches: ahead/behind count, last-commit date, origin-presence, branch name pattern.
2. Batch branches (10–11 per agent) and embed the pre-computed facts so agents spend time on judgment, not redundant git calls.
3. Each agent runs `git cherry -v master <branch>` (all-`-` = merged, any `+` = unmerged commits), inspects commit subjects, and checks whether the branch diverged from a post-podcast-deletion epoch.
4. Each agent returns a structured verdict table: branch, verdict (MERGED / OBSOLETE / KEEP / UNCERTAIN), one-line evidence.
5. Orchestrator aggregates verdicts, performs a second-pass spot-check on high-value KEEP and suspicious OBSOLETE calls before acting. [^752b5-14]

## Squash-Merge Detection

`git branch -d` (safe mode) refuses branches whose HEAD is not reachable from master via ancestry. Squash-merged branches pass `git cherry` all-`-` even though git ancestry doesn’t show them merged. The definitive test for squash-merged branches is `git cherry -v master <branch>` — all `-` markers means every commit's patch-id exists in master, regardless of how it got there. [^752b5-15]

## Origin-Presence as Safety Gate

Classify every delete candidate by whether it exists on `origin`:

- **On origin**: deleting the local ref is loss-free. The remote ref and all commits are recoverable via `git fetch`. Delete confidently.
- **Not on origin**: deletion is **irreversible**. Apply a higher evidence bar. A Haiku-only "unmerged" verdict is insufficient; verify that the branch’s distinctive changes are byte-identical in master before deleting.

This distinction is the single most important safety gate in the triage workflow. [^752b5-16]

## Known Haiku Over-Rating Patterns

First-pass Haiku agents reliably over-rate large-ahead-count branches:

- **High commit count ≠ unmerged work**: A branch 800+ commits ahead with no merge-base to current master is almost certainly a divergent orphan from a past epoch, not unmerged production work.
- **`+` in `git cherry` on old bases**: Cherry output uses patch-id matching; if the branch base predates the merge, cherry will show `+` for commits that are semantically present in master via a different commit path.
- **Orphaned branches with no merge-base**: Branches with root commits not reachable from master origin cannot be merged or rebased onto master without manual conflict resolution. Always flag these as UNCERTAIN/OBSOLETE pending deeper verification.

Always spot-check Haiku OBSOLETE verdicts for not-on-origin branches before deleting. [^752b5-17]

## See Also

