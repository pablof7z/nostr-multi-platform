---
title: Direct Development Must Use Git Worktrees — Never the Main Checkout
slug: worktree-required-for-direct-development
summary: All direct development work must be performed in a git worktree, never on the main git checkout.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:ecf13381-c8ef-40bf-9498-04a1d1f2af8f
---

# Direct Development Must Use Git Worktrees — Never the Main Checkout

> All direct development work must be performed in a git worktree, never on the main git checkout.

## Requirement

All direct development work must be performed in a git worktree, not on the main git checkout. The main tree must be preserved in a clean state on master. This applies to both human-directed and agent-driven work. [^ecf13-23]

## Worktree Agent Pattern

When launching a sub-agent to perform development work, the agent is spawned in an isolated git worktree on a dedicated branch (e.g., fix/desktop-keyring-and-projections). The agent then builds, tests, and opens a PR from that worktree branch. This isolates the work from the main tree and enables parallel development. [^ecf13-24]

## Branch and PR Flow

The correct sequence is: create worktree on a descriptive branch, perform all changes in the worktree, commit locally, push to the branch, and open a PR. The worktree is pruned after the PR is merged. The main checkout remains on master throughout. [^ecf13-25]

## See Also
- [[agent-push-to-master-violation|Sub-Agents in Worktrees Must Push to Branch and Open a PR — Never Push to Master]] — related guide
- [[disk-pressure-kills-agent-fleet|Accumulated Worktrees Cause Disk Exhaustion — Prune After Every Merge]] — related guide
- [[main-checkout-violation-recovery|Main Checkout Violation — Recovery When Agent Works in Wrong Tree]] — related guide

