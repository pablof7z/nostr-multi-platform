---
title: Agent Workflow Conventions
slug: agent-workflow-conventions
topic: developer-workflow
summary: All implementation work must happen in a git worktree; open a ready-for-review PR when work is complete and clean up the worktree afterward
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edc01-fdde-7b20-a348-5a2a9ce1a0f9
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:019edc0c-2dd1-7b80-b737-7499340e1b49
  - session:019edc13-83b1-7143-8631-b0e695ea4afd
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edc4d-4175-7441-b5af-cb2012068335
  - session:019edc63-ed50-7dc0-9f1a-38e311efc3b4
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
  - session:019edcba-b578-71f3-be33-f670962f11a7
---

# Agent Workflow Conventions

## Agent Workflow Conventions

When the user gives a correction about how the NMP product should work, a separate agent must research whether the correction should be represented in product specs, doctrine, canonical docs, GitHub Issues, or ADRs before making code changes. Before designing a solution for critical items, agents must get a codex exec solution to design the problem. Completed work must be opened as a ready-for-review pull request (not a draft), and the agent-owned worktree must be cleaned up after the PR is opened. The PR description must include a TLDR summary, a detailed overview, and any subjective decisions including tradeoffs or assumptions. All implementation agents use isolated worktrees; the base directory stays on master. A master-branch monitor watches the base directory and wakes the orchestrator the instant the base directory leaves master so offending agents can be redirected. As much parallel work as possible is fanned out within the safety constraint of preventing agents from stepping on each other. To enforce this, work is partitioned into file-disjoint lanes so no two agents edit the same file; hot files have exactly one owner. If an agent's fix would cross a lane boundary, the agent must stop and report rather than touch another lane's files. The coordinator only intervenes if an agent reports a cross-lane blocker or if the master-branch monitor trips. Agents waiting on CI to complete should not be interrupted or nudged if they are actively watching and reporting progress on their own. Tactical state and release-plan checkpoints have a single source of truth: GitHub Issues. State must not be duplicated across files; a violation or feature tracked in GitHub Issues is not restated elsewhere. GitHub search is the backlog view; use gh issue list to enumerate open issues by priority.

<!-- citations: [^019ed-18] [^129d2-35] [^019ed-39] [^019ed-66] [^11850-6] [^019ed-84] [^11850-48] [^019ed-95] [^11850-68] [^019ed-125] [^11850-182] [^019ed-148] -->
