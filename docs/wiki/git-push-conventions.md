---
title: Git Push Conventions
slug: git-push-conventions
summary: Every push must rebase onto origin/master first, retry once on non-fast-forward, and never force-push.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-28
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:e50d12a1-0a49-4a45-9bb1-251fa0f434b6
  - session:ad1d532e-a335-44fb-827e-a3f0318a3aae
  - session:f7021d71-aadd-4666-a266-a033744efd77
  - session:3afdf0df-923b-46cb-8fa6-acc61358bb75
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:d98be997-81df-4738-8846-8323d40ab9ff
  - session:f5503f3a-d44c-4626-b8de-0492ad1f2a6c
---

# Git Push Conventions

## Push Conventions

All PRs must rebase from origin/master before starting work. Agent branches use `git push origin HEAD:<branch-name>` (not `git push origin master`), and force-pushed rebased branches use `--force-with-lease`. Changes are pushed to a new branch with a fresh PR instead of force-pushing to an existing branch. (Previously: Never force-push to PR branches — always use `git merge` forward, resolve conflicts, and normal push.) The master push protocol requires running `git fetch origin master && git rebase origin/master && git push origin HEAD:master`, retrying once on failure; never force-push to master. Always verify master's actual state with `git diff HEAD origin/master` before declaring master broken, to avoid false alarms from local stash residual. origin/master and master are always kept in sync. The main repo checkout must be periodically fast-forwarded with `git fetch origin && git merge --ff-only origin/master` because codex creates worktrees from the main repo HEAD and a stale HEAD produces a wrong baseline. When a local `git merge origin/master --ff-only` fails due to active worktrees, the workaround is to use `gh pr merge --repo` for server-side merge only. When `WIP.md` or `BACKLOG.md` conflict during a rebase, the resolution must preserve content from both sides (both the HEAD entries and the incoming branch entries). When rebasing a branch with release manifest conflicts where both master and the PR delete the same lines, the PR's version (which includes its new additions) takes precedence. When multiple PRs modifying BACKLOG.md land simultaneously causing merge conflicts, the resolution is to rebase the later PR onto origin/master, resolve the conflict manually, force-push with `--force-with-lease`, then merge. Commits for the desktop shell use the prefix feat(desktop):. All work must be merged to master rather than left in local working trees. All PRs are merged with `gh pr merge --squash` (not `--auto`, which fails because branch protection lacks `enablePullRequestAutoMerge`). `gh run rerun` reruns the workflow on the exact same git SHA, not the branch HEAD; to trigger a new pull_request workflow run, a new commit must be pushed to the PR branch.

<!-- citations: [^f7021-2] [^e50d1-6] [^ad1d5-9] [^3afdf-1] [^423f3-5] [^12b3f-5] [^1c093-7] [^45258-9] [^f2605-6] [^d98be-4] [^f5503-2] -->
## See Also

