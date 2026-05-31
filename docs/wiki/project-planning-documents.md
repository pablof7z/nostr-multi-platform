---
title: Project Planning Documents — plan.md, BACKLOG.md, and WIP.md
slug: project-planning-documents
summary: "The project uses three live planning documents with non-overlapping roles:  - `docs/plan.md` — The canonical durable overview document"
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
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:44c6cebb-bea4-4ca7-b836-0337e090a2a5
---

# Project Planning Documents — plan.md, BACKLOG.md, and WIP.md

## Canonical Planning Documents

The project uses three live planning documents with non-overlapping roles. Plan docs must not scatter around the repo; only `docs/plan.md`, `docs/BACKLOG.md`, and `WIP.md` are canonical planning files:

- `docs/plan.md` — The canonical durable overview document. Covers milestones, doctrine, current state mapping the M0–M17 ladder to master, v1 exit criteria, post-v1 list, working agreements, and pointers to supporting docs. (Previously: `docs/plan.md` was stale and behind the codebase, with Marmot, NWC, NIP-57, and negentropy all built but still marked as future milestones; it was reconciled with the current state from `BACKLOG.md` and `status.md`.) The plan must be delivered as a PR document, not as code changes. Steps 1–4 (backend) must be a single PR with zero shell changes; steps 5–6 must be a separate chirp-tui PR.
- `docs/BACKLOG.md` — The tactical queue. Tracks active violations, pending user decisions, and the ordered v1 feature backlog.
- `WIP.md` — The live in-flight status tracker for branches currently in flight. Its role must be preserved (it was incorrectly marked as superseded).

Active plans are kept in `docs/`; implemented plans are deleted. Within `docs/plan/`, implemented milestone plans (M0–M10) are deleted, while future milestone plans (M12+) are kept. All `docs/design/`, `docs/decisions/`, and `docs/builder-guide/` directories are kept.

<!-- citations: [^9fc44-1] [^fbebb-6] [^f2605-19] [^44c6c-3] -->
## Planning Discipline in AGENTS.md

`AGENTS.md` must contain a planning discipline section enforcing that `plan.md`, `BACKLOG.md`, and `WIP.md` are the three canonical planning files with no duplicates, no new top-level plan files, no duplicated state, plan files outranking scattered notes, single source of truth per fact (doctrine D4 applied to docs), edit-in-place rather than append-parallel, and fewer files when in doubt — violations are rejected and folded back. [^9fc44-2]

## CLAUDE.md as Thin Pointer

`CLAUDE.md` must serve as a thin pointer to `AGENTS.md` with a cold-start reading order and TL;DR of the planning discipline, intentionally avoiding content duplication to demonstrate the no-duplication rule. [^9fc44-3]
## See Also

