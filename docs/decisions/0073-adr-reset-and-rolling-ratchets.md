# ADR-0073: ADR reset and rolling architecture ratchets

## Status

Accepted for the architecture redesign direction.

## Context

The ADR directory has accumulated many accepted point decisions. Several protect
current invariants. Others preserve milestone-specific or defaults-era guidance
that now conflicts with the desired architecture. Issue #2320 states the source
of truth correction: old ADRs that no longer describe accepted architecture
should be folded, corrected, deleted, or retired. They should not survive as
parallel current guidance.

The redesign direction also must not become a big-bang rewrite plan. The
architecture is valid only if each slice reduces permanent concepts, public
doors, lifecycle recipes, shell policy sites, or duplicate owners while
preserving behavior.

## Decision

ADR-0069 through ADR-0073 are the current redesign spine:

- ADR-0069 owns explicit app composition and app/product policy boundaries.
- ADR-0070 owns typed read sessions and private read machinery disposition.
- ADR-0071 owns write intent, event construction/signing/publishing separation,
  and route provenance.
- ADR-0072 owns runtime, capability, and shell boundaries.
- ADR-0073 owns the ADR reset and rolling ratchet discipline.

Older ADRs remain authoritative only for current invariants that do not conflict
with this redesign spine. When an older ADR teaches a superseded app-facing
surface, its current rule must be amended in place or the ADR must be folded,
deleted, or retired by the PR touching that area. Git history preserves decision
history; the active ADR directory should not act as a museum of old public
architecture.

The ADR index classifies each older ADR as one of:

- folded into the redesign spine;
- folded into another durable owner such as architecture docs, product spec, or
  builder guide;
- still-current standalone invariant;
- retired/deleted because it is milestone-specific, stale, or superseded.

The temporary `docs/new-arch/` candidate packet has been retired. Future
architecture proposals must move surviving decisions into ADRs, durable docs,
and GitHub issues instead of leaving a parallel architecture packet behind.

Migration proceeds by rolling horizon. At any point, only the next one to five
PR-sized slices need implementation-level detail. After each slice, the project
must recount old surfaces, record what was deleted/privatized/scoped, verify
ratchets, and choose the next slice.

## Consequences

Positive:

- New contributors get one current architecture spine instead of dozens of
  contradictory accepted records.
- The project can aggressively delete stale docs without pretending a full
  migration can be planned up front.
- Progress is measured by shrinking old surfaces and passing ratchets, not by
  landing prettier terminology.

Negative/tradeoffs:

- The cleanup is continuing work: many references in product specs, builder
  guides, recipes, wiki pages, and older ADRs must be corrected in place as
  implementation slices touch those areas.
- Some historical detail will move out of active docs and into git history.
- The ADR index is not permission to keep stale guidance forever. Folded and
  amended ADRs should shrink when their implementation area is next edited.

## Alternatives considered

| Option | Why rejected |
|---|---|
| Keep all old ADRs as accepted and add a new appendix | It preserves conflicting current guidance and violates the single-source rule. |
| Delete old ADRs before accepting the redesign spine | It risks removing useful invariants before their new owner is clear. |
| Plan the entire migration trajectory now | It would become stale and distract from PR-sized proof slices. |
| Measure success by fewer files only | Fewer files can hide worse ownership. The real metric is fewer public nouns, doors, recipes, and sources of truth. |

## Fitness functions / enforcement

- `docs/decisions/README.md` owns the ADR classification ledger.
- Every remaining ADR is either current standalone guidance or has a clear folded
  owner.
- Stale references to production `register_defaults()`, app-facing
  `open_interest`, public `ReducedSource`, public `ObservedProjection`, hidden
  projection tiers, and anonymous explicit relay routes are corrected in place
  or classified as compatibility with owner/removal trigger.
- Architecture slices include a deletion ledger:

```text
old public doors deleted or privatized:
old compatibility paths scoped:
new public concepts added:
net permanent concepts:
```

- Ratchets prevent old-pattern counts from increasing before broad migration is
  complete.

## Linked work

- #2320: original ADR reset issue, now closed after the spine and index landed.
- #2316: foundational feature-state decomposition.
- #2313: app-developer API complexity.
- #2324: landed the initial ADR reset/spine cleanup.
