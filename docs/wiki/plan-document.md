---
title: Canonical Plan & Tracking Documents
slug: plan-document
summary: The three canonical planning files are plan.md, BACKLOG.md, and WIP.md
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-05-29
verified: 2026-05-23
compiled-from: conversation
sources:
  - session:9fc44c34-8e49-4959-91b3-714d4722ac3d
  - session:e3b42d41-ffd2-44b3-9e5a-93832feb46e0
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:44c6cebb-bea4-4ca7-b836-0337e090a2a5
  - session:1d30779f-b6ee-44ad-a1f1-bdc17f26ebdd
---

# Canonical Plan & Tracking Documents

## Canonical Planning Files

The three canonical planning files are docs/plan.md, docs/BACKLOG.md, and WIP.md. No new top-level plan files may be created; plan or investigation files outside these three locations must be corrected before commit. docs/plan.md serves as the canonical overarching plan file containing milestones, doctrine, current state, and pointers to supporting documents; it must not contain hardcoded links to codex-review files. BACKLOG.md and WIP.md must be kept up to date properly, and plan docs must not scatter around the repo. WIP.md is the live in-flight status tracker for branches currently in flight and must not be marked as superseded; it retains its role as the live in-flight tracker. WIP.md must not be committed to the repository, must be listed in .gitignore, and must be edited directly in the main repo rather than via PR. (Previously: WIP.md was committed to the repository.) Stale docs/plan/ files for already-landed features should be deleted, but future milestone plans in docs/plan/m12+ must be kept. A stray-file check is performed after each merge to catch any agent-created plan or investigation files outside the three canonical locations.

The docs/ directory taxonomy is sound and follows the AGENTS.md planning-discipline structure, requiring no sweeping reorganization. [^1d307-3]

docs/wiki/_index.md is a derived-but-navigable index that is tracked in git. [^1d307-4]

<!-- citations: [^9fc44-5] [^e3b42-2] [^f2605-11] [^44c6c-3] -->
## Single Source of Truth

Planning state must not be duplicated across files; a single source of truth per fact (D4 applied to docs) is required. Plan files outrank scattered notes; existing canonical files must be edited in-place rather than appending parallel copies. PRs that violate the planning discipline rules must be rejected and folded back. [^9fc44-6]

## Agent and Onboarding Files

AGENTS.md and CLAUDE.md must explain what those files are and mandate that all plans and tasks are written in the proper canonical files with strict repository discipline and no duplicated plan files or scattered notes. CLAUDE.md serves as a thin pointer to AGENTS.md with a cold-start reading order and TL;DR, intentionally avoiding content duplication. [^9fc44-7]
## See Also

