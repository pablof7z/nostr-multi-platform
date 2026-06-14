---
title: Git Worktree Policy
slug: git-worktree-policy
topic: codebase-patterns
summary: Agents must work in isolated git worktrees, never moving the base repo away from master.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:019ec57a-fb01-7081-80c8-d7107f302049
---

# Git Worktree Policy

## Git Worktree Policy

All implementation work must happen in a git worktree owned by the agent doing the work, not in the shared root checkout. Before starting work, every agent must read WIP.md to understand what other agents are currently doing. When an agent starts work, it must add an entry to WIP.md with a timestamp, a one-line description, and the git worktree path. When an agent finishes work, it must remove its own entry from WIP.md. After opening a PR, the agent must clean up its owned worktree before completing the task. Stash entries must be reviewed against branches/PRs and either attached to an appropriate branch/worktree, converted into a proper commit/PR path, or dropped only after verifying they are obsolete or duplicated. When merging master forward into a PR branch, WIP.md conflicts are resolved by keeping master's active tracker state and dropping the stale PR-specific WIP entries.

<!-- citations: [^02745-100] [^02745-118] [^02745-130] [^019ec-16] -->
## Breaking Changes

The standing rule is: always do the right thing on breaking changes, never hedge on migrations, manually upgrade NMP consumer apps (podcast-player/hl/win-the-day) by git-rev bump rather than scheduling or staging. <!-- [^02745-119] -->
