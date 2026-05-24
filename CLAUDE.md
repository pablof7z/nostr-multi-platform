# CLAUDE.md

This file intentionally defers to [`AGENTS.md`](AGENTS.md), which is the canonical contributor guide for this repository. Every rule that applies to agents applies to Claude (and vice versa). Keeping both files in sync would violate the repository's own planning-discipline rule (single source of truth per fact) — so `AGENTS.md` is authoritative and this file is a pointer.

## ⚠ ARCHITECTURE FREEZE — DO NOT START NEW WORK

A crate-boundary architecture synthesis is in progress (2026-05-24). The output will be
`docs/architecture/crate-boundaries.md`. Until that document exists and the freeze is
lifted, **no agent may start any new feature or refactor work**. Agents may only:
- Fix a CI red that is blocking an open PR
- Answer a direct user question without touching files

The freeze exists because the synthesis will decide which crates own which responsibilities.
Work started now may be in the wrong crate and will need to be redone. Check WIP.md for
status.

## Cold-start reading order

1. [`AGENTS.md`](AGENTS.md) — repository conventions, planning discipline (three canonical files), doctrine corollaries, agent workflow, file-size rules.
2. [`docs/aim.md`](docs/aim.md) — immutable architectural north star.
3. [`docs/plan.md`](docs/plan.md) — overarching plan, milestone ladder vs. actual state, v1 exit criteria.
4. [`docs/BACKLOG.md`](docs/BACKLOG.md) — active violations, pending user decisions, ordered v1 feature backlog, post-v1 list.
5. [`WIP.md`](WIP.md) — work currently on a branch.

## Planning discipline — TL;DR

Three canonical files: `docs/plan.md` (overview), `docs/BACKLOG.md` (queue), `WIP.md` (in-flight). No new top-level plan files, no scattered todo lists, no parallel roadmaps. Full rules in [`AGENTS.md`](AGENTS.md#planning-discipline--three-canonical-files-no-duplicates).
