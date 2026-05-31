---
title: Sub-Agents in Worktrees Must Push to Branch and Open a PR — Never Push to Master
slug: agent-push-to-master-violation
summary: Worktree sub-agents must always push to their own branch and open a PR; direct pushes to master are forbidden.
tags:
  - git
  - worktree
  - ci
  - agents
  - workflow
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Sub-Agents in Worktrees Must Push to Branch and Open a PR — Never Push to Master

> Sub-agents are launched inside git worktrees, each isolated to a dedicated branch. The correct exit sequence is always: commit locally, push to the worktree branch, then open a PR. Pushing directly to `master` from a worktree agent bypasses review, breaks branch protection, and corrupts the shared history.

## Details


In the chirp cross-platform session (batch 1), agents followed AGENTS.md push protocol and pushed directly to master instead of the integration branch `chirp-cross-platform`. The work landed correctly but bypassed the review gate. The protocol was corrected for batch 2: agents create `feature/<task_id>` branches and the Sonnet merge agent cherry-picks onto the integration branch. [^f3d8d-18]
### Correct Sequence
```
git commit -m "..."
git push origin HEAD:worktree-agent-<id>
gh pr create --base master --head worktree-agent-<id> --title "..." --body "..."
```

### Why This Matters
- Worktrees share the same `.git` object store; a direct push to `master` from any worktree immediately affects all other agents and the main checkout.
- Branch protection rules (required reviews, status checks) are bypassed by a direct push.
- PRs provide the audit trail required for backlog citation and architectural review.

### Common Mistake
An agent that finishes work and runs `git push origin HEAD:master` (or `git push origin master`) instead of pushing to its own branch. Always double-check the refspec before pushing.

### Naming Convention
Worktree branches should follow the pattern `worktree-agent-<id>` or a descriptive slug agreed upon at agent launch time. The branch name must be passed to `gh pr create --head`.


### Additional Rule

Confirmed violation: agent afe8fb2ecf6bf7447 fixed GH #615 (backoff reset) and pushed commit 5da5942c directly to master instead of its worktree branch. This rule applies regardless of how clean, well-tested, or trivial the change appears. There are no exceptions for 'obvious' fixes.

### Additional Rule

Confirmed again in GH #615 fix (commit 5da5942c): an agent pushed directly to master even though the change was clean and tests passed. The rule is unconditional — `gh pr create` is always required regardless of change size or test status. There are no exceptions for 'trivial' fixes.

## Additional Rule

A second class of violation was also observed: a worktree agent switched the main checkout's branch (not just pushing to master, but actually changing `HEAD` on the main checkout via git operations run against the wrong cwd). The main checkout must always remain on `master`; orchestrators should verify `git branch --show-current` on the main checkout after completing each agent batch. [^42908-56]
## See Also
- [[wip-md-is-untracked|wip md is untracked]] — related guide
- [[disk-pressure-kills-agent-fleet|Accumulated Worktrees Cause Disk Exhaustion — Prune After Every Merge]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[loop-command|/loop — Recurring and Self-Paced Prompt Scheduling]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide
- [[worktree-required-for-direct-development|Direct Development Must Use Git Worktrees — Never the Main Checkout]] — related guide
- [[main-checkout-violation-recovery|Main Checkout Violation — Recovery When Agent Works in Wrong Tree]] — related guide
