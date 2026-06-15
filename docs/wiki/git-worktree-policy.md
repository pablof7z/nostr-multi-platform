---
title: Git Worktree Policy
slug: git-worktree-policy
topic: codebase-patterns
summary: All implementation work must happen in a git worktree owned by the agent doing the work, not from the shared root checkout
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:019ec57a-fb01-7081-80c8-d7107f302049
  - session:fabf8ca3-e1b9-4a7c-bcd5-bf5731fb571d
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
---

# Git Worktree Policy

## Git Worktree Policy

All implementation work must happen in a git worktree owned by the agent doing the work, not from the shared root checkout. No work may be lost during worktree or stash cleanup operations. Before starting work, every agent must read WIP.md to understand what other agents are currently doing. When an agent starts work, it must add an entry to WIP.md with a timestamp, a one-line description, and the git worktree path. When an agent finishes work, it must remove its own entry from WIP.md. After opening a PR, the agent must clean up its owned worktree before completing the task. Git worktrees and stashes must be reviewed and cleaned up. Locked worktrees must never be removed during cleanup. Worktrees with active concurrent agent edits must be excluded from removal. A safety bundle of all refs must be created before any destructive cleanup operations. Parallel read-only agents are dispatched to investigate branch status while deletions are performed centrally to avoid race conditions. Stash entries must be reviewed against branches/PRs and either attached to an appropriate branch/worktree, converted into a proper commit/PR path, or dropped only after verifying they are obsolete or duplicated. Dropped stashes must be backed up as patch files before deletion. Abandoned but valuable uncommitted work must be taken on, verified, and landed. Branches that differ from master but might be superseded by a differently-implemented merged PR must be preserved for user review rather than auto-landed. Unverified branches beyond the worktree set must not be bulk-deleted without confirmation. When merging master forward into a PR branch, WIP.md conflicts are resolved by keeping master's active tracker state and dropping the stale PR-specific WIP entries.

<!-- citations: [^02745-100] [^02745-118] [^02745-130] [^019ec-16] [^fabf8-3] [^019ec-46] -->
## Breaking Changes

The standing rule is: always do the right thing on breaking changes, never hedge on migrations, manually upgrade NMP consumer apps (podcast-player/hl/win-the-day) by git-rev bump rather than scheduling or staging. <!-- [^02745-119] -->
