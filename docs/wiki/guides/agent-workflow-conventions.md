---
title: Agent Workflow Conventions
slug: agent-workflow-conventions
topic: developer-workflow
summary: Before starting work, every agent must read WIP.md from the project base directory to understand what other agents are currently doing
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

Before starting work, every agent must read WIP.md from the project base directory to understand what other agents are currently doing. When an agent starts work, it must add an entry to WIP.md with a timestamp, a one-line description, and the git worktree path. When an agent finishes work, it must remove its own entry from WIP.md. When the user gives a correction about how the NMP product should work, a separate agent must research whether the correction should be represented in product specs, doctrine, canonical docs, docs/plan.md, GitHub Issues, or ADRs before making code changes. Before designing a solution for critical items, agents must get a codex exec solution to design the problem. Completed work must be opened as a ready-for-review pull request (not a draft), and the agent-owned worktree must be cleaned up after the PR is opened. The PR description must include a TLDR summary, a detailed overview, and any subjective decisions including tradeoffs or assumptions. All implementation agents use isolated worktrees; the base directory stays on master. A master-branch monitor watches the base directory and wakes the orchestrator the instant the base directory leaves master so offending agents can be redirected. As much parallel work as possible is fanned out within the safety constraint of preventing agents from stepping on each other. To enforce this, work is partitioned into 8 file-disjoint lanes so no two agents edit the same file; hot files have exactly one owner. If an agent's fix would cross a lane boundary, the agent must stop and report rather than touch another lane's files. PR #1525 (snapshot-projector) overlaps the P1 area; the P1 agent must coordinate with it. Session 10f152 owns nmp-store/nmp-nostr-lmdb (epic #1523 on cache/store); all other agents must not touch those crates. The coordinator only intervenes if an agent reports a cross-lane blocker or if the master-branch monitor trips. Agents waiting on CI to complete should not be interrupted or nudged if they are actively watching and reporting progress on their own. Tactical state, in-flight branch ownership, and release-plan checkpoints each have a single source of truth: GitHub Issues, WIP.md, and docs/plan.md respectively. State must not be duplicated across files; a violation or feature tracked in GitHub Issues is not also restated as a queue row in WIP.md or docs/plan.md — only the branch reference and issue number live in WIP.md. GitHub search is the backlog view; use gh issue list to enumerate open issues by priority, skipping issues whose number appears in WIP.md unless explicitly coordinating with that agent. Issues #1558 and #1563 must be rewritten before assigning agents. The landing order is: #1554 first, then #1557, then #1555, #1556, #1558-minimal once rewritten/ADR'd, merging one ABI/schema PR at a time. Wave 3/4 work must be deferred until wave 1/2 actually unblocks hl; do not blindly run the whole tracker.

<!-- citations: [^019ed-18] [^129d2-35] [^019ed-39] [^019ed-66] [^11850-6] [^019ed-84] [^11850-48] [^019ed-95] [^11850-68] [^019ed-125] [^11850-182] [^019ed-148] -->
