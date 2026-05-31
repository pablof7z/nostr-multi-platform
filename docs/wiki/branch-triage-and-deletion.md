---
title: Branch Triage & Deletion Policy
slug: branch-triage-and-deletion
summary: Branch triage uses parallel Haiku agents for read-only git investigation with conservative classification so genuine work is never tagged for deletion
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
  - session:629ffcce-f1dc-44e7-9b6e-8c6166d571bb
---

# Branch Triage & Deletion Policy

## Branch Triage and Deletion

Branch review and triage is performed by Haiku agents running in parallel batches, using conservative classification so genuine work is never tagged for deletion. Haiku agents classify branches into KEEP, MERGED, and OBSOLETE categories by examining cherry counts, diff stats, and actual diffs, with extra scrutiny on branches not on origin. PRs are automatically opened for all branches classified as KEEP before any worktree cleanup occurs. Only assistant agents perform branch deletions; triage agents never delete branches. Branches not present on origin are held for explicit user approval before deletion because deletion would be irreversible. git branch -d (safe mode) is used for bulk merged-branch deletions because git automatically refuses any branches checked out in a live worktree or not actually merged. Worktrees with live PIDs are preserved and not removed during cleanup. Worktrees with dead PIDs are not treated as empty or discardable; their branch contents must be reviewed for valuable work before any cleanup. Removal of locked worktrees requires a double --force flag. A branch with a large ahead/behind commit count does not necessarily indicate unmerged valuable work; ADR-0027 branches were 866-commit divergent orphans whose features had already shipped piecemeal via PRs. [^752b5-2]

<!-- citations: [^752b5-2] [^629ff-1] -->
## See Also

