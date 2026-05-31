---
title: Main Checkout Violation — Recovery When Agent Works in Wrong Tree
slug: main-checkout-violation-recovery
summary: "Recovery procedure when an agent accidentally works in the main checkout: verify content on master, rebuild branch cleanly via cherry-pick, restore main to master."
tags:
  - worktree
  - recovery
  - incident
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Main Checkout Violation — Recovery When Agent Works in Wrong Tree

> Recovery procedure when an agent accidentally works in the main checkout: verify content on master, rebuild branch cleanly via cherry-pick, restore main to master.

## The Incident

A Haiku agent (V-68 orphan file deletion) accidentally worked in the main checkout instead of its worktree. The agent moved the main checkout's HEAD off master and bundled 80 stray docs/wiki/ files (~5,600 lines) that were committed by a peer agent into the PR alongside the intended 2-file deletion. The PR showed 2 deletions + .gitignore (intended) + 80 new wiki files (unintended scope drift). [^4edd4-117]

## Recovery Procedure

The clean recovery procedure: (1) Verify the stray content is already on origin/master — if so, it's not being lost. In this case, the wiki commit's content was byte-identical to what was already on origin/master via a different hash. (2) Rebuild the branch in a throwaway worktree as master + cherry-pick the clean commit only. (3) Force-push the cleaned branch to the PR. (4) Handle untracked files in the main checkout that block git checkout back to master — back them up to /tmp if they contain differing content, remove duplicates. (5) Restore the main checkout to master. (6) Verify the cleaned PR diff shows only the intended changes. [^4edd4-118]

## Untracked File Handling

When restoring the main checkout to master, untracked files (e.g., PNG screenshots) may block the checkout. Run git stash --include-untracked to clear them out. First verify there are zero tracked changes via git diff --stat (no uncommitted tracked work to lose). Back up any files that differ from the tracked version before removing them, since binary diffs always report "differ" and may represent regenerated artifacts with value. [^4edd4-119]

## Preferred Approach — Cherry-Pick Into Throwaway Worktree

When the main checkout's HEAD has moved off master, the safest recovery avoids working in the main checkout entirely. Instead: create a throwaway worktree at master, cherry-pick the clean commit(s) from the dirty branch, force-push the cleaned branch, then separately restore the main checkout to master. This avoids the main checkout's untracked-file mess and ensures the operation is reversible at every step. [^4edd4-120]

## Verification After Recovery

After recovery, verify: (1) the PR diff shows exactly the intended changes (no scope drift), (2) the main checkout is back on master and in sync with origin/master, (3) any backed-up files are preserved if they had differing content, (4) the cleaned worktree is pruned. In this case, the V-68 PR went from 82 files changed to exactly 2 deletions after recovery. [^4edd4-121]

## See Also
- [[worktree-required-for-direct-development|Direct Development Must Use Git Worktrees — Never the Main Checkout]] — related guide
- [[agent-push-to-master-violation|Sub-Agents in Worktrees Must Push to Branch and Open a PR — Never Push to Master]] — related guide

