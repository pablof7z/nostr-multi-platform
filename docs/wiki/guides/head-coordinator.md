---
title: Head Coordinator and PR Workflow
slug: head-coordinator
topic: head-coordinator
summary: The head-coordinator merges clean pull requests
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
---

# Head Coordinator and PR Workflow

## Responsibilities

The head-coordinator merges clean pull requests. Agents open ready pull requests and do not merge them themselves. A worktree is only removed if its HEAD is a strict ancestor of origin/master (fully merged, zero unique commits), has no uncommitted tracked changes, is not locked, and is not on an active ns-* or recent branch; anything with unique or unpushed commits is preserved and reported, never deleted. Orphan branches with unique unpushed commits must be mirrored to origin/backup/* branches to prevent work loss before their worktrees are removed.

<!-- citations: [^3c942-074b6] [^3c942-41a0a] -->
