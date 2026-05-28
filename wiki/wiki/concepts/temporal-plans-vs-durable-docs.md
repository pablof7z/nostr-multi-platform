---
title: "Temporal Plans vs Durable Docs"
summary: "Plans coordinate current work; durable NMP understanding belongs in specs, ADRs, design docs, code, tests, and source-backed wiki articles."
tags: [docs, planning, authority]
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
volatility: warm
confidence: high
sources:
  - "raw/notes/2026-05-28-temporal-plans-correction.md"
  - "raw/repos/2026-05-28-source-map.md"
---

# Temporal Plans vs Durable Docs

Plans in NMP are coordination artifacts. They answer "what are we doing now",
"what is queued", and "what worktree owns this branch". They are not the place
to preserve durable architecture, product doctrine, or implementation lessons
after a plan has been executed.

The durable result of work belongs in the document that owns the concept:
doctrine in `docs/product-spec/`, architecture in `docs/architecture/`, design
contracts in `docs/design/`, decisions in `docs/decisions/`, builder-facing
how-to material in `docs/builder-guide/`, and implementation truth in code and
tests.

## Coordination Files

The temporal coordination files are:

- `docs/plan.md`: current release-plan view and v1 exit criteria.
- `docs/BACKLOG.md`: active violations, pending user decisions, and queued work.
- `WIP.md`: live worktrees and branches.

These files can describe active state, but completed detail should be removed
or reduced to the smallest remaining live follow-up.

## Durable Sources

Durable docs should read as stable explanation, not as preserved project
management. A durable doc can say "the runtime update transport is FlatBuffers"
and explain why. It should not preserve an old task ladder after the ladder was
implemented.

The wiki sits on the durable side, but only as derived synthesis. It may explain
what current durable sources say. It must not become a hidden backlog, roadmap,
or decision registry.

## See Also

- [[source-authority-map|Source Authority Map]] ([Source Authority Map](../references/source-authority-map.md))
- [[crate-boundaries-and-module-ownership|Crate Boundaries and Module Ownership]] ([Crate Boundaries and Module Ownership](../topics/crate-boundaries-and-module-ownership.md))

## Sources

- [Temporal Plans Product Correction](../../raw/notes/2026-05-28-temporal-plans-correction.md)
- [NMP Source Map 2026-05-28](../../raw/repos/2026-05-28-source-map.md)
