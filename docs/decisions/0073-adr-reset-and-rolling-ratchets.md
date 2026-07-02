# ADR-0073: ADR directory governance and rolling ratchets

## Decision

`docs/decisions/` contains only decisions that still govern NMP architecture.

One invariant has one owner. When a decision stops describing the architecture,
any surviving rule moves to the owner that now governs it, and the obsolete ADR
file is deleted. Git history, closed GitHub issues, and pull request bodies
preserve prior context.

The current decision spine is:

- [ADR-0069](0069-explicit-feature-composition.md) - explicit composition and
  app-owned product policy.
- [ADR-0070](0070-typed-read-sessions.md) - app-visible read lifecycles.
- [ADR-0071](0071-write-intents-and-route-provenance.md) - write intents,
  drafts, signing/publishing separation, and route provenance.
- [ADR-0072](0072-runtime-capability-and-shell-boundary.md) - runtime,
  capability, and shell ownership.
- ADR-0073 - this directory governance rule.

Architecture cleanup proceeds through PR-sized slices. Each slice must reduce or
hold the line on old public doors, duplicate lifecycle recipes, shell policy
sites, stale docs, and extra sources of truth. The issue queue remains the
tactical tracker; ADRs and durable docs carry only rules that need to outlive a
single task.

## Context

An ADR directory that keeps every old point decision teaches competing
architecture. NMP needs a small, readable decision surface that helps a future
agent choose the present owner instead of archaeology through stale public APIs.

The cleanup cannot be a one-time prose exercise. Ratchets are needed so old
public surfaces do not return after the docs are cleaned.

## Consequences

Deleting obsolete ADR files makes links and references stricter: stale guidance
must move to an owner or disappear. That creates reference-update work in the
same PR, but it removes parallel authority.

New ADRs carry a higher burden. If an existing ADR or durable doc can own the
rule, update that owner instead of creating another packet.

## Boundaries

Permitted:

- editing a current ADR in place when its rule changes;
- moving reference material into architecture, product, API, design, perf, or
  testing docs;
- deleting obsolete ADR files once surviving rules have an owner;
- linking issues and PRs for context in `Related`.

Forbidden:

- tombstone ADR files;
- status tables or lifecycle ledgers in the ADR README;
- parallel architecture packets;
- ADRs that are implementation diaries, migration notes, API manuals, or
  release logs;
- links that tell readers to decide which ADR wins.

## Enforcement

Reviewers reject ADR changes that add lifecycle sections, tombstones, status
tables, long chronology, or stale links. Reference checks fail when Markdown
links point to deleted ADR files or when raw ADR mentions cite missing numbers.

Clean-room docs ratchets prevent production docs and starter templates from
teaching hidden defaults, raw reads, public read internals, anonymous explicit
routes, raw native framework bindings, or public Trellis concepts.

## Related

- [ADR-0069](0069-explicit-feature-composition.md)
- [ADR-0070](0070-typed-read-sessions.md)
- [ADR-0071](0071-write-intents-and-route-provenance.md)
- [ADR-0072](0072-runtime-capability-and-shell-boundary.md)
- [docs/architecture/high-level-app-architecture.md](../architecture/high-level-app-architecture.md)
- #2746 - ADR current-only cleanup.
