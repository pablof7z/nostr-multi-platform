---
title: WIP.md Entries Are Frequently Stale — Verify Branch/PR Status Before Acting
slug: stale-wip-entries-common
summary: WIP.md tracks in-flight work but entries often outlive their branches; always verify against git before treating WIP.md as authoritative.
tags:
  - wip
  - git
  - workflow
  - discovery
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# WIP.md Entries Are Frequently Stale — Verify Branch/PR Status Before Acting

> WIP.md is a manually maintained file that tracks in-flight branches and their status. Because it is untracked by git, it is never automatically updated when branches are merged, deleted, or superseded. Entries routinely become stale. Always cross-reference against the live git state before treating WIP.md as a source of truth.

## Details

### Why Entries Go Stale
- Branches are merged and deleted but WIP.md is not updated.
- PRs are closed (merged or abandoned) without a corresponding WIP.md edit.
- Worktree agents cannot update the root WIP.md directly (see `wip-md-is-untracked`).
- Orchestrators forget to clean up entries after task completion.

### Verification Commands
```bash
# Check if a branch still exists remotely
git ls-remote --heads origin <branch-name>

# Check PR status
gh pr list --head <branch-name>

# Check if a branch was already merged into master
git log --oneline master | grep <commit-or-keyword>
```

### Safe Interpretation Rules
- A WIP.md entry marked "in progress" may already be merged — verify.
- A WIP.md entry with a branch name may point to a deleted branch — verify.
- Never block work or skip a task solely because WIP.md says it is "in progress" by another agent.

### Cleanup Obligation
When you confirm an entry is stale, update WIP.md at the root (not in a worktree) to remove or resolve it.


### Additional Rule

A 50-agent audit found that WIP.md entries dated 2026-05-24/25 were stale: step-8 phases B/C/D were marked in-flight but had all been merged to master. Multiple branches listed as 'Active' were fully merged. Treat WIP.md as a high-staleness artifact — any entry older than a few days should be considered suspect until verified against git log and GitHub PR status.

### Additional Rule

Audit example: multiple entries marked 'in-flight' (step 8 phases B/C/D, feat/step-12-nmp-marmot-return-to-crates) had been fully merged weeks earlier with no open PRs remaining. Standard verification procedure: run `git branch -r | grep <branch-name>` and `gh pr list --search <branch-name>` for every WIP entry before treating it as active work.
## See Also
- [[wip-md-is-untracked|wip md is untracked]] — related guide
- [[wip-md-is-untracked|wip md is untracked]] — related guide
- [[pd-decisions-can-be-stale-in-backlog|pd decisions can be stale in backlog]] — related guide
- [[backlog-citations-must-match-head|BACKLOG.md Violation Entries Must Cite File:Line Verified Against Current HEAD]] — related guide
- [[disk-pressure-kills-agent-fleet|Accumulated Worktrees Cause Disk Exhaustion — Prune After Every Merge]] — related guide
