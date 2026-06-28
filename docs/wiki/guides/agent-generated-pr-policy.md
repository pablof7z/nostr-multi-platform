---
title: Agent-Generated PR Policy
slug: agent-generated-pr-policy
topic: developer-workflow
summary: All sub-issues must be worked one by one and landed in master via PRs, using Opus agent to plan, Sonnet agent to code, and codex exec to review â no technical
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-19
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:1ca92577-a656-4fd9-879e-0f2fd87f0ee7
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edbff-1d29-7533-99ab-0b8130b805dc
  - session:019edc16-8e40-7a92-9ea1-7405af0d34f3
  - session:019edc59-7035-7ba3-95cc-789d362adff2
  - session:019edc84-6e5c-74a2-9ed9-57938dae31a1
  - session:019edc94-e2f8-76e3-8cdc-a6d8f6bba72a
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
---

# Agent-Generated PR Policy


## Agent-Generated PR Policy

All sub-issues must be worked one by one and landed in master via PRs, using Opus agent to plan, Sonnet agent to code, and codex exec to review — no technical debt, no hacks, perfect engineering. Epic #1523 is closed with all 9 sub-issues merged to master: #1515 (docs), #1522 (baselines), #1517 (coverage audit), #1516 (streaming query_visit), #1518 (relay×kind index), #1520 (event-driven wakeups), #1521 (LMDB diagnostics), #1524 (acceptance gates), #1519 (interaction counters). All agents must work on isolated worktrees so the base directory stays on master, and workflows must be fanned out in parallel whenever possible. Where a fix would cross a lane boundary, the agent must stop and report rather than touching another lane's files. Critical lanes must get a codex design-first review before implementation; all lanes must get codex review before PR. Critical items must get a codex exec solution design before designing a fix: points 5, 1, 2, 3 and P5, P7, P4. All agents must run codex review on their diff and get a codex problem/solution design first for critical items before implementing; actionable findings from the review must be promoted to a GitHub issue or durable doc, and the review artifact itself must be discarded — never committed to the repository. Each agent squash-merges its own PR to master after CI green and codex review clean, and must delete its remote branch after merge; the orchestrator performs merge sweeps for green-but-unmerged PRs since agents that come to rest after launching CI do not auto-wake when GitHub CI finishes. Completed work must be opened as a ready-for-review pull request (not a draft), and the PR description must include a TLDR summary, a detailed overview of the work performed, and any subjective decisions or tradeoffs. After opening a PR, the agent must clean up its worktree before completing the task. Draft PRs are used only when explicitly requested or when the work is intentionally incomplete. P9 is granted full vertical ownership (option A) for its three coupled breaking changes, including actor/mod.rs, dispatch.rs, apps/chirp, apps/nmp-gallery, crates/nmp-cli, crates/nmp-ffi builder wiring, schema/codegen/shell files. Codex review before PR is a mandatory gate for all remaining campaign PRs; the 3 already-merged P1 PRs had post-hoc verification only (no review-doc commits), which caught a real nmp-app-chirp E0560 bug missed by both manual review and CI. Post-hoc codex review is acceptable only for already-merged PRs as a verification pass, not as a substitute for the pre-merge gate. A monitor watches the base directory HEAD and wakes the orchestrator if it leaves master, so misdirected agents can be redirected.

<!-- citations: [^129d2-1] [^129d2-4] [^129d2-13] [^11850-1] [^019ed-1] [^019ed-56] [^11850-5] [^11850-8] [^129d2-74] [^019ed-91] [^11850-47] [^129d2-106] [^11850-67] [^11850-92] [^019ed-106] [^129d2-121] [^019ed-113] [^019ed-117] [^11850-112] [^019ed-124] [^11850-130] [^11850-181] [^11850-224] [^129d2-129] [^129d2-136] -->
