# CLAUDE.md

This file intentionally defers to [`AGENTS.md`](AGENTS.md), which is the canonical contributor guide for this repository. Every rule that applies to agents applies to Claude (and vice versa). Keeping both files in sync would violate the repository's own planning-discipline rule (single source of truth per fact) — so `AGENTS.md` is authoritative and this file is a pointer.

## Architecture migration in progress

The canonical crate-boundary spec lives at
[`docs/architecture/crate-boundaries.md`](docs/architecture/crate-boundaries.md). The
prior architecture freeze (2026-05-24) is lifted now that the spec exists. Migration
follows the 12-step order in §5 of that document. New work that touches crate boundaries
must align with that plan; ad-hoc moves are out of bounds. Check `WIP.md` for what is
currently in-flight.

## Cold-start reading order

1. [`AGENTS.md`](AGENTS.md) — repository conventions, planning discipline (three canonical files), doctrine corollaries, agent workflow, file-size rules.
2. [`docs/aim.md`](docs/aim.md) — immutable architectural north star.
3. [`docs/plan.md`](docs/plan.md) — temporal release plan, milestone ladder vs. actual state, v1 exit criteria.
4. [`docs/BACKLOG.md`](docs/BACKLOG.md) — active violations, pending user decisions, ordered v1 feature backlog, post-v1 list.
5. [`WIP.md`](WIP.md) — work currently on a branch.

## Planning discipline — TL;DR

Three canonical temporal files: `docs/plan.md` (release-plan view), `docs/BACKLOG.md` (queue), `WIP.md` (in-flight). Plans are coordination artifacts, not durable understanding; implemented plan detail is removed or moved into durable docs. No new top-level plan files, no scattered todo lists, no parallel roadmaps. Full rules in [`AGENTS.md`](AGENTS.md#planning-discipline--temporal-files-no-duplicate-plans).

## Test scope — TL;DR

Run `cargo test` scoped to the crates you touched (`cargo test -p
<crate>`), plus `cargo test -p nmp-testing --test doctrine_lint_smoke`
always. Do **not** run `cargo test --workspace` — that's reserved for
the supervisor at merge time. Workspace-wide runs serialize the cargo
build queue across parallel worktrees and starve other agents. Full
rules in [`AGENTS.md`](AGENTS.md#test-scope--local-vs-ci-vs-merge).
