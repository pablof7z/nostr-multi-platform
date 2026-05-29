---
title: WIP.md Is Untracked — Worktree Agents Cannot Update It Directly
slug: wip-md-is-untracked
summary: WIP.md lives at the repo root and is untracked by git; worktree agents must manually sync any changes after merge.
tags:
  - wip
  - git
  - worktree
  - workflow
  - config
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# WIP.md Is Untracked — Worktree Agents Cannot Update It Directly

> WIP.md is intentionally excluded from git tracking and lives only at the repo root of the main checkout. Because worktrees do not share the working-tree files of the main checkout, any WIP.md edits made inside a worktree are invisible to the canonical location and will be lost unless manually synced.

## Details

### File Status
- `WIP.md` is listed in `.gitignore` (or otherwise untracked).
- It does not appear in `git status`, `git diff`, or any commit.
- It is **not** copied into new worktrees created with `git worktree add`.

### Implication for Worktree Agents
- A sub-agent working in `worktrees/<name>/` that writes to `WIP.md` is writing to `worktrees/<name>/WIP.md`, which is a separate file from the root `WIP.md`.
- After the worktree branch is merged, the root `WIP.md` is unchanged.
- Any intended WIP.md updates must be applied manually to the root after merge.

### Recommended Pattern
If a worktree agent needs to record WIP state:
1. Write the intended WIP.md diff to a temporary file or PR description.
2. After merge, a human or orchestrator agent applies the diff to the root `WIP.md`.

### Treating WIP.md as a Source of Truth
See the companion guide on stale WIP entries — even the root `WIP.md` can be stale after merges.

## See Also
- [[stale-wip-entries-common|stale wip entries common]] — related guide
- [[agent-push-to-master-violation|Sub-Agents in Worktrees Must Push to Branch and Open a PR — Never Push to Master]] — related guide
