---
title: Worktree Removal — Check Live Agent PIDs Before Force-Removing Locks
slug: worktree-live-agent-pid-check
summary: Before force-removing a locked worktree, check whether the locking PID is still alive; removing a live agent's worktree destroys in-flight work.
tags:
  - worktrees
  - agents
  - safety
  - git
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
---

# Worktree Removal — Check Live Agent PIDs Before Force-Removing Locks

> Before force-removing a locked worktree, check whether the locking PID is still alive; removing a live agent's worktree destroys in-flight work.

## Overview

Before force-removing any locked worktree, read the lock file to extract the PID and verify whether the locking process is still alive. Removing a worktree held by a live agent destroys its in-flight work. [^752b5-9]

## Check Live PIDs Before Force-Removing

Worktree lock files record the agent PID. Before running `git worktree remove --force <path>` on any locked worktree:

1. Read the lock file: `cat .git/worktrees/<name>/locked`
2. Check whether the PID is alive: `kill -0 <pid>` (exit 0 = alive, non-zero = dead)
3. Remove only if the PID is dead (stale lock)
4. Leave any worktree whose locking PID is still running — even if the branch is already merged

A live Claude agent process creates a lock on its worktree. That process may still be writing files, committing, or pushing. Pulling its worktree out from under it causes silent data loss. [^752b5-10]

## Worktree-Agent Orphan Branches Are Heartbeat-Managed

Branches named `worktree-agent-*` are created and retired by the project heartbeat process. Do **not** manually delete them. The heartbeat cherry-picks commits from orphan `worktree-agent-*` branches as part of its normal operation; deleting them breaks that pipeline.

Even branches that show as "fully merged" in `git branch -d` output should not be deleted from the `worktree-agent-*` namespace unless the heartbeat has explicitly retired them. [^752b5-11]

## Pruning Dead Worktrees Safely

A worktree directory can disappear (e.g., manually removed or cleaned by the OS temp sweep) while its admin entry in `.git/worktrees/` still exists. Run `git worktree prune` to remove stale admin entries for directories that no longer exist. This is always safe and has no side effects on live worktrees. [^752b5-12]

## See Also
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide

