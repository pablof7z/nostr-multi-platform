---
title: "NMP Source Map 2026-05-28"
summary: "Durable NMP source surfaces used to seed the committed repo wiki."
tags: [repo, sources, architecture]
source_type: repo-snapshot
repo: /Users/pablofernandez/Work/nostr-multi-platform
commit: 372a6ddb914e8ff8fc90c507d42928c34bbd96da
ingested: 2026-05-28
updated: 2026-05-28
---

# NMP Source Map 2026-05-28

## Durable Authority Sources

- `AGENTS.md`: contributor workflow, temporal planning discipline, worktree rules, Rust/native boundary, no polling, no hacks.
- `docs/aim.md`: architectural north star, TEA actor model, Rust-owned core, framework purpose.
- `docs/product-spec/doctrine.md`: durable D0-D10 doctrine statements.
- `docs/architecture/crate-boundaries.md`: durable crate-layer and ownership rules.
- `docs/design/`: design contracts for reactivity, subscription compilation, transport, and extension boundaries.
- `docs/decisions/`: ADRs for binding, projection, routing, and transport decisions.
- `docs/builder-guide/`: maintained builder-facing explanations of current behavior.
- Source crates under `crates/` and app crates under `apps/`: implementation truth.

## Temporal Sources

- `docs/plan.md`: current release-plan view and v1 exit criteria.
- `docs/BACKLOG.md`: active violations, pending decisions, and feature backlog.
- `WIP.md`: live branches and worktrees.

These temporal sources may be cited for current status, but wiki articles must
not treat executed plan detail as durable understanding.

## First Compile Focus

The first committed wiki compile focused on:

- plan temporality and documentation authority;
- Rust-owned logic versus native shell boundaries;
- actor update loop and FlatBuffers runtime transport;
- subscription planning and routing;
- crate/module ownership;
- source lookup for future agents.
