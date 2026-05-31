---
title: Background Agent Worktrees
slug: background-agent-worktrees
summary: Background agents run `git rev-parse --show-toplevel` first and only work in their own worktree, never the primary checkout.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:ad1d532e-a335-44fb-827e-a3f0318a3aae
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:156aa64b-42e1-4d3b-96ce-25b31fc06fec
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:7174d4d4-371b-4b8e-87a6-91024c2b4c2a
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:d98be997-81df-4738-8846-8323d40ab9ff
  - session:9de494e6-e783-4785-ae67-1f7014dadd5d
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
---

# Background Agent Worktrees

## Git Worktree Isolation

All work is performed via subagents; the orchestrator only orchestrates and never implements directly. Every agent must use `isolation: "worktree"` and stay within their provided worktrees — agents must never work in the main directory. Changes to master are made via worktrees and PRs, not by touching the master checkout directly. When checking out branches for testing or development, a git worktree is used instead of navigating away from the master branch. Agents commit to their worktree branch and open a PR — they never push to master directly; the orchestrator merges. The orchestrator never runs `git checkout`, `git stash`, or branch-switching operations on the main checkout; it stays on master at all times. Background agents run `git rev-parse --show-toplevel` first and only work in their own worktree, never the primary checkout. Background agents can be launched asynchronously using the Agent tool with `run_in_background: true`, and the caller is automatically notified upon completion. The desktop gallery work occurs on a git worktree. Backend (Steps 1-4) and TUI (Steps 5-6) can be built in parallel in the same git worktree because they touch completely disjoint files. Agents must always use `git push origin HEAD:<branch-name>` (not `git push origin <branch-name>`) and must always pass `run_in_background: true`. Git worktrees are cleaned up after their work is merged. Locked worktrees held by live agent processes must not be removed to avoid destroying in-flight work. In a shared environment, worktree bulk-force-removal must not be performed without checking ownership first. Stale locked worktrees are force-removed with `git worktree remove -f -f` and then the branch is deleted with `git branch -D`. Orphan worktree-agent-* branches are left untouched because the heartbeat system manages them. Stashed work from a prior session must be investigated; approximately 1300 lines of unstaged changes appeared in the main tree that may have been written by an agent that wrote to main instead of its worktree.

<!-- citations: [^1c093-1] [^ad1d5-7] [^12b3f-1] [^156aa-1] [^1670f-1] [^45258-3] [^53838-1] [^7174d-1] [^f2605-1] [^d98be-1] [^9de49-3] [^54ae9-1] [^42908-1] [^752b5-1] -->
## Memory and PR Fixes

Memory must be fixed in a background agent by purging superseded reviews and adding code-grounded entries. PRs A, B, C must all be fixed in background agents using the documented PR approach in AGENTS.md. [^1c093-2]
## See Also

