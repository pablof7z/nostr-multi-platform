---
title: Head Coordinator and PR Workflow
slug: head-coordinator
topic: head-coordinator
summary: All changes go through a proper worktree/branch and PR workflow â nothing is committed directly to the main checkout
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-03
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:04745411-a0c1-4523-ac83-71dc983f410b
  - session:1c293d33-5ec2-4689-b6c2-cd159d8b6bb7
---

# Head Coordinator and PR Workflow

## Responsibilities

All changes go through a proper worktree/branch and PR workflow — nothing is committed directly to the main checkout. The head-coordinator merges clean pull requests; agents open ready pull requests and do not merge them themselves.

The root checkout is always kept in sync with origin/master. A worktree is only removed if its HEAD is a strict ancestor of origin/master (fully merged, zero unique commits), has no uncommitted tracked changes, is not locked, and is not on an active ns-* or recent branch; locked and dirty worktrees are preserved during cleanup. Anything with unique or unpushed commits is preserved and reported, never deleted. Orphan branches with unique unpushed commits must be mirrored to origin/backup/* branches to prevent work loss before their worktrees are removed.

The #2580 cleanup policy removes clean+merged worktrees, deletes merged local branches, reaps merged remote branches via a delegated agent, and keeps the root checkout synced. Remote branch reaping only touches merged-PR branches and posts a full plan to the issue before deleting anything. Unproven local branches (no-PR or closed-unmerged) and unproven remote branches are kept for human triage per the safety rules, not auto-deleted.

When multiple PRs are in flight, the head-coordinator sequences merges to avoid branch collisions: land a foundational PR before dispatching fresh agents onto dependent live WIP branches. For example, PR #2869 (the wallet journal/trail design of record) is still open with failing CI (UNSTABLE) and must be merged before journal implementation work proceeds; dispatching a fresh agent at the live WIP branch #2871 risks a collision, so the safest sequencing is to land #2869 first and then have one agent continue #2871.

The Phase 1 wallet implementation is taken as a spine-first slice rather than one PR for all of Phase 1, because a single PR for the entirety of Phase 1 would be a huge, hard-to-review money-handling change.

Wallet implementation agents work in isolated worktrees on dedicated branches with open PRs and never push directly to master; the worktree is cleaned up when done.

Before touching a live WIP branch, wallet agents check the PR (e.g. #2871) for active @codex pushes; if a concurrent push is detected, they report a collision rather than fighting it.

NMP wallet work must be done in an isolated worktree, on a branch with a PR (never push to master), with the worktree cleaned up when done. NMP's gh branch delete chokes when the agent's worktree still holds the branch, so merge sweeps must delete branches remote-only. <!-- [^91a86-4ba29] -->

<!-- citations: [^91a86-da265] [^91a86-5b44e] [^91a86-bd2f0] [^3c942-074b6] [^3c942-41a0a] [^04745-867b9] [^91a86-5d300] -->
## PR Review Process

When reviewing a PR plan proposal, the head-coordinator spawns a Fable subagent for a second-pair-of-eyes review pass before publishing anything on the PR. Once all analysis and the second-pass review are complete, the head-coordinator publishes comments, corrections, and implementation plans on the PR. In the case of PR #2854, the wallet architecture design proposal was already merged to master at the time of review, making the review post-merge. PR #2888 adopted a pre-existing nearly-complete draft from a sibling worktree rather than opening a competing PR, after verifying its citations were accurate and confirming it was more complete than the independently-written version.

<!-- citations: [^91a86-8789f] [^91a86-63652] [^1c293-5d502] -->
