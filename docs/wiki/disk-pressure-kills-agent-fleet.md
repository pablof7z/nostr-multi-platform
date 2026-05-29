---
title: Accumulated Worktrees Cause Disk Exhaustion — Prune After Every Merge
slug: disk-pressure-kills-agent-fleet
summary: Parallel agent fleets can exhaust disk (each Rust worktree is 2–5 GB); prune worktrees immediately after merging PRs.
tags:
  - worktree
  - disk
  - agent-fleet
  - operations
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:ae88711c-a987-4b41-939e-32c8ee0ab4d3
  - session:ecf13381-c8ef-40bf-9498-04a1d1f2af8f
---

# Accumulated Worktrees Cause Disk Exhaustion — Prune After Every Merge

> Each git worktree that contains a full Rust build consumes 2–5 GB of disk. When running parallel agent fleets, worktrees accumulate quickly. In one incident, 26 accumulated worktrees consumed 59 GB, leaving only 1.6 GB free and killing 4 of 5 running agents mid-task.

## Details

- **After merging any agent PR**, immediately run `git worktree remove --force <path>` and delete the associated branch.
- **Before launching a large parallel fleet**, run `df -h` and `git worktree list` to audit current disk usage. Ensure at least 5–10 GB free per planned agent.
- **Rust build artifacts** are the primary space consumer; `cargo clean` inside a worktree before removal is optional but can reclaim space faster if disk is already tight.
- **Worktree count discipline**: keep active worktrees ≤ number of concurrently running agents. Merged-but-not-pruned worktrees are pure waste.
- **Symptom of disk exhaustion**: agents die with I/O errors, `cargo build` fails with 'no space left on device', or git operations fail silently mid-commit.


**Worktree lock files can prevent removal**: when a worktree dies mid-task, it may leave a stale `.git/worktrees/<name>/locked` file. Use `git worktree remove --force <path>` to bypass the lock. If that also fails, manually remove the worktree directory and then run `git worktree prune` to clean the `.git/worktrees/` metadata. [^42908-57]

Details

When a long-running orchestrator process holds locks on all large worktrees simultaneously, worktree pruning is not possible — find space elsewhere. In one incident, a single PID 43960 (a 4.5-hour-old claude process) held locks on every large worktree. The recovery freed 1.5 GB from caches to create enough headroom for merge operations. The verification sequence: (1) check df -h to confirm severity, (2) run git worktree list to inventory worktrees and their lock PIDs, (3) for each locked worktree, read the lock file and verify whether the PID is alive via kill -0, (4) identify whether one PID holds locks on multiple worktrees — this indicates a long-running orchestrator that must not be touched, (5) check target dir modification times to confirm the orchestrator is actively writing, (6) free space from safe caches rather than touching any worktree locked by a live PID. [^ae887-59]

When /tmp is full, Bash tools cannot write temporary output files and operations fail silently. This can cause file edits to appear to succeed but not actually land on disk — the edit tools may report success while the underlying filesystem write never occurred. After clearing disk pressure, always re-verify file contents on disk before building or committing. [^ecf13-12]
## See Also
- [[agent-push-to-master-violation|agent push to master violation]] — related guide
- [[stale-wip-entries-common|stale wip entries common]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[loop-command|/loop — Recurring and Self-Paced Prompt Scheduling]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide
- [[worktree-required-for-direct-development|Direct Development Must Use Git Worktrees — Never the Main Checkout]] — related guide

- agent-push-to-master-violation
- stale-wip-entries-common
