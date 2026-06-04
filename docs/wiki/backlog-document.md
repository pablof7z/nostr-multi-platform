---
title: Backlog Document Synthesis
slug: backlog-document
summary: docs/BACKLOG.md is the tactical queue; active branch coordination lives only in WIP.md, and this wiki page is non-authoritative synthesis.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-06-04
verified: 2026-06-04
compiled-from: conversation
sources:
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:9fc44c34-8e49-4959-91b3-714d4722ac3d
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:bbd5fe79-cd71-4de0-ba9f-f3684520a03f
---

# Backlog Document Synthesis

## Overview

This wiki page summarizes `docs/BACKLOG.md`; it does not replace it.
`docs/BACKLOG.md` is the tactical queue for active violations, pending user
decisions, ordered v1 feature work, and post-v1 deferrals. Completed work is
removed or collapsed to the smallest live follow-up, with durable conclusions
moved to the document that owns the concept.

## Coordination Boundary

Active branch/worktree coordination lives in `WIP.md`, not in a BACKLOG
section. Agents use `WIP.md` to avoid duplicating in-flight work, then edit
`docs/BACKLOG.md` only when the queue item itself changes.

## Durable Use

Backlog entries should cite current code when they assert live violations.
Review findings are promoted into BACKLOG only when they identify actionable
work; committed review dumps, direction reviews, and implementation plans are
not durable backlog material.

## Related Files

- `AGENTS.md` owns planning discipline.
- `docs/plan.md` owns the temporal release-plan view.
- `WIP.md` owns ignored live branch/worktree status.
- `docs/perf/pending-user-decisions.md` is append-only history, not the queue.
