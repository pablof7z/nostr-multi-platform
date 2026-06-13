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
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Git Worktree Policy

## Git Worktree Policy

Agents must work in isolated git worktrees, never moving the base repo away from master.

<!-- citations: [^02745-100] [^02745-118] [^02745-130] -->
## Breaking Changes

The standing rule is: always do the right thing on breaking changes, never hedge on migrations, manually upgrade NMP consumer apps (podcast-player/hl/win-the-day) by git-rev bump rather than scheduling or staging. <!-- [^02745-119] -->
