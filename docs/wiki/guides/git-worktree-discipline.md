---
title: Git Worktree Discipline
slug: git-worktree-discipline
topic: developer-workflow
summary: All worktrees, orphan checkout directories, and stale branches (including worktree-*, codex-review-*, push-tmp, wip-snapshot-hb42, codex/worker1-nip17-dm-inbox-
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-06-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:d5c1c624-3c9d-4fa7-a910-84bd59c75724
  - session:9998902d-7260-4b24-9f29-a84f8eded0b5
  - session:64c4fde3-6f5e-456a-b4bb-9f17517e301c
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:5a40faff-56c9-442d-ad96-59432b6f2fea
  - session:e3b42d41-ffd2-44b3-9e5a-93832feb46e0
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edbff-1d29-7533-99ab-0b8130b805dc
  - session:019edc01-fdde-7b20-a348-5a2a9ce1a0f9
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
---

# Git Worktree Discipline

## Worktree Removal Discipline

All worktrees, orphan checkout directories, and stale branches (including worktree-*, codex-review-*, push-tmp, wip-snapshot-hb42, codex/worker1-nip17-dm-inbox-relays, etc.) must be cleaned up and removed from the repository. The repository's steady state is a single master branch with no leftover worktrees or stale branches. Worktrees must not be removed until a git push is confirmed; removing worktrees early caused multiple issues during the milestone. Before removing any stale branch, confirm it is dead (e.g., codex/worker1-nip17-dm-inbox-relays is stale — work already landed on master via PR #237 + PR #300). After opening a PR, the agent must clean up its owned worktree before completing the task.

Agent worktrees must be dedicated (not shared); a shared worktree exhibited stale git index.lock, branch-switching under active agents, and ~35 uncommitted files from other lanes.

Per-worktree target/ directories consume multiple GB each (up to 16 GB for the shared worktree), causing disk exhaustion on a 926G volume; agents must cargo clean their worktrees after each PR merges to prevent disk exhaustion on the build host.

A violation or feature tracked in GitHub Issues must not also be restated as a queue row in docs/plan.md; the issue is the queue authority.

<!-- citations: [^11850-33] [^e3b42-3] [^d27a4-1] [^d5c1c-1] [^95d02-7] [^019ed-12] [^019ed-21] [^11850-70] -->
## Branch Landing and Sync Discipline

The main checkout at `/Users/pablofernandez/Work/nostr-multi-platform` must never have `git checkout`, `git stash`, or any branch-switching operation performed on it — it must stay on `master` at all times. All branch work happens in isolated worktrees. All implementation work must happen in a git worktree owned by the agent doing the work; agents must not edit from the shared root checkout for feature, fix, or refactor work. All implementation work must be done in a git worktree and PR'd+merged into master, never committed directly to the main checkout.

All agents must use isolated worktrees and fan out parallel workflows whenever possible.

All PRs must use git push origin HEAD:feat/<branch-name> protocol; never push directly to master except for test+doc only changes.

Each agent works in its own git worktree, never touches the main branch directly, and squash-merges its own PR to master after CI goes green and codex reviews the diff.

All branches must be merged to master and pushed. The local master branch must be kept in sync with origin, rebasing onto any new remote commits before pushing.

Before codex runs, the main repo local checkout must be fast-forwarded with `git fetch origin && git merge --ff-only origin/master`, because codex creates worktrees from the main repo HEAD and a stale HEAD gives a wrong baseline.

Uncommitted work on the master branch must be committed and synced with origin/master promptly.

A Chirp TUI binary does not exist on master branch; TUI work lives in a chirp-tui-spec worktree branch.

The worktree `worktree-agent-a2725f5d233151e13` has a D0 fix commit renaming `chirp.follow/unfollow` → `nmp.follow/nmp.unfollow` that needs a PR opened.

Agents commit to worktree branches and open PRs; never push directly to master; the orchestrator merges.

Git push --force must never be used; after squash-merge divergence, the correct pattern is to delete the remote branch then push fresh.

GitHub `refs/pull/{N}/merge` is NOT recomputed on PR close/reopen without a new push; a push to the PR branch is required to force GitHub to recompute the test merge ref and trigger `pull_request` events.

PR #582 was merged at d73e048b preserving the 6-commit history (original transport switch + 4 hardening commits), with the codex/flatbuffers-transport remote branch deleted and local master fast-forwarded.

A master-branch monitor must trip the instant the base dir's HEAD leaves master, re-invoking the orchestrator with the offending branch + status + reflog.

<!-- citations: [^11850-3] [^11850-4] [^45258-12] [^45258-13] [^45258-14] [^64c4f-1] [^d5c1c-2] [^99989-1] [^95d02-8] [^53838-5] [^5a40f-1] [^e4861-15] [^f2605-8] [^56d21-5] [^019ed-11] [^11850-12] [^129d2-89] [^11850-208] -->
