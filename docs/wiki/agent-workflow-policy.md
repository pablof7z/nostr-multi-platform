---
title: Agent Workflow Policy
slug: agent-workflow-policy
topic: codebase-patterns
summary: Completed work must be opened as a ready-for-review pull request (not a draft) unless explicitly asked or intentionally incomplete; the PR description must incl
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-15
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:019ec57a-fb01-7081-80c8-d7107f302049
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:c9a794f6-6ad7-4ee9-a620-fc342fd495c3
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
  - session:019eca83-d126-7fc3-9bd7-e83f65c0a643
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# Agent Workflow Policy

## Pull Request Standards

Completed work must be opened as a ready-for-review pull request (not a draft) unless explicitly asked or intentionally incomplete; the PR description must include a TLDR summary, detailed overview, and any subjective decisions or tradeoffs. After opening a PR, the agent must clean up its owned worktree before completing the task. Direct orchestration — assigning one implementer per rung and merging via a background gh-checks waiter — proved effective, landing roughly 15 PRs cleanly. Delegating to 'lead' agents stalled due to checkpoint-and-exit behavior, duplicate-ownership conflicts, and CI-monitor waits, and should be avoided. The agent will not self-merge PRs regardless of coordinator requests; standing instructions reserve merge authority for the user/owner. Technical debt accumulation is absolutely forbidden; deferring things for later because it is more comfortable is itself technical debt. Decisions should be made by measuring when possible; when in doubt, get codex exec feedback and research rather than hand-waving.

<!-- citations: [^019ec-1] [^2e544-361] [^78c8e-461] [^78c8e-480] [^78c8e-489] [^019ec-20] [^019ec-27] [^019ec-41] -->
## Product Corrections

When a user gives a product correction or instruction, a separate agent must research whether it should be represented in product specs, doctrine, canonical docs, `docs/plan.md`, GitHub Issues, or ADRs before making code changes; documentation updates must be in the same PR as the implementation unless explicitly scoped to docs only.

<!-- citations: [^019ec-2] [^019ec-21] -->
## Agent Orchestration Loop

Use the opus+codex+sonnet+opus-review loop: opus for planning/design, codex for feedback/research, sonnet for coding, and opus for review. Before implementation proceeds, an opus agent must review accepted proposals to determine whether it agrees with them or considers them hacks. Parallelism should be limited.

Before starting work, every agent must read WIP.md from the project base directory to understand what other agents are currently doing. When an agent starts work, it must add an entry to WIP.md with a timestamp, a one-line description of the work, and the git worktree path it is using. When an agent finishes work, it must remove its own entry from WIP.md. GitHub search via gh issue list is the backlog view; skip issues whose number appears in WIP.md unless explicitly coordinating with that agent.

<!-- citations: [^019ec-1] [^78c8e-481] [^c9a79-11] [^019ec-42] -->
## Planning and Issue Hygiene

GitHub Issues are the one canonical tactical queue; scattered planning files (TODO.md, NOTES.md, ROADMAP.md, PLAN-foo.md) and inline TODO comments used as tracking substitutes are forbidden. Plans must not survive as reference documentation after being implemented; lasting knowledge belongs in durable docs (`docs/aim.md`, `docs/product-spec/`, `docs/architecture/`, `docs/design/`, `docs/decisions/`, builder guide, `wiki/`). New top-level planning files at the repo root or directly under `docs/` are forbidden; tactical detail belongs in a GitHub issue, and durable decisions belong in `docs/decisions/00NN-*.md` ADRs. Temporal coordination artifacts like `docs/plans/arch-fixes.md` — which carry a 'delete when merged, move detail to ADR + wiki' instruction — are not durable roadmaps and must not be treated as such. Workstreams tracked in plan files must be triaged against current master and open PRs, then filed as priority-labeled GitHub issues rather than kept in the plan file. A violation or feature tracked in GitHub Issues must not be duplicated in `WIP.md` or `docs/plan.md`; if an agent is fixing it on a branch, only the branch reference and issue number live in `WIP.md`. Issue priority labels define work order: `priority:p0` through `priority:p4`; within a bucket, `category:violation` comes before `category:feature`, then `category:test`, then `category:decision`. Existing issues must be edited in place rather than appending parallel ones; executed plans must be retired or replaced with the smallest remaining live follow-up issue; preserve durable lessons in the durable doc that owns that concept.

<!-- citations: [^019ec-22] [^019ec-28] [^78b50-61] [^78b50-72] -->
