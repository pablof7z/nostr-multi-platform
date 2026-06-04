---
title: Planning Authority Documents
slug: plan-document
summary: AGENTS.md defines the planning-authority split: plan.md is the temporal release view, BACKLOG.md is the tactical queue, and WIP.md is ignored live branch coordination.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-06-04
verified: 2026-06-04
compiled-from: conversation
sources:
  - session:9fc44c34-8e49-4959-91b3-714d4722ac3d
  - session:e3b42d41-ffd2-44b3-9e5a-93832feb46e0
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:44c6cebb-bea4-4ca7-b836-0337e090a2a5
  - session:1d30779f-b6ee-44ad-a1f1-bdc17f26ebdd
---

# Planning Authority Documents

## Synthesis Boundary

This wiki page is synthesis, not authority. The authority is `AGENTS.md`
§Planning discipline. It defines exactly three temporal coordination files:
`docs/plan.md`, `docs/BACKLOG.md`, and the ignored live `WIP.md` tracker.
Durable understanding belongs in product specs, architecture/design docs,
ADRs, builder guides, source code, or source-backed wiki articles.

## Roles

- `docs/plan.md` is the current release-plan view. It is temporal and should
  delete or collapse implemented detail.
- `docs/BACKLOG.md` is the tactical queue for active violations, pending user
  decisions, v1 feature work, and post-v1 deferrals.
- `WIP.md` is live branch/worktree coordination. It is listed in `.gitignore`
  and should not be committed.

## Single Source of Truth

Planning state must not be duplicated. Active branch coordination belongs only
in `WIP.md`; queued work belongs in `docs/BACKLOG.md`; release-plan state
belongs in `docs/plan.md`. A review, audit, or implementation plan may surface
a finding, but the review artifact itself is not durable documentation.

## Agent and Onboarding Files

`AGENTS.md` is the contributor-guide authority. `CLAUDE.md` defers to it.
Wiki pages should point back to those files rather than restating the workflow
as if the wiki were a second planning system.
