---
title: "NMP Threading Crate: ThreadPointer, ParentResolver & Grouper"
slug: nmp-threading
summary: The nmp-threading crate is a new sibling crate depending only on nmp-core, shipping the ThreadPointer, ParentResolver trait, ModulePolicy, TimelineBlock, and Gr
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-26
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:6e6bcf78-bf6b-4ddd-a2b8-4fb829d86604
---

# NMP Threading Crate: ThreadPointer, ParentResolver & Grouper

## Crate Identity

The nmp-threading crate is a new sibling crate depending only on nmp-core, shipping the ThreadPointer, ParentResolver trait, ModulePolicy, TimelineBlock, and Grouper algorithm. [^423f3-11]


## ThreadPointer

ThreadPointer is a three-variant enum (Event, Address, External) defined in nmp-threading, forced by NIP-22's addressable a-tag and I-tag parents, with NIP-10's case being the degenerate Event variant. Non-Event ThreadPointers (Address, External) terminate the ancestor walk since there is no id to hydrate. The TUI renders threads as a depth-indented flat view rather than a tree pane, because Nostr threads are DAGs (NIP-10), not trees. [^423f3-12]

<!-- citations: [^423f3-12] [^4f377-18] -->
## ParentResolver

The threading grouper is reply-convention-agnostic via a ParentResolver trait, allowing NIP-10 and NIP-22 to be pluggable without the app knowing which protocol it is using. nmp-nip01 ships Nip10Resolver and Nip10ModularTimelineView, and nmp-nip22 ships Nip22Resolver and Nip22ModularTimelineView as thin per-NIP ViewModule wrappers. The ParentResolver trait provides a kind-agnostic `supersedes(event) -> Option<EventId>` hook (defaulting to `None`) allowing protocol-specific resolvers to declare that one event displaces another in the block layout. The Nip10Resolver in nmp-nip01 overrides `supersedes` for kind:6 reposts, using `nmp-nip18::try_from_kernel_event` to extract and return the target event ID.

<!-- citations: [^423f3-13] [^6e6bc-2] -->
## Grouper Spec

The grouper crate is source-agnostic and has no TimelineSource enum; Spec declares ViewDependencies, ParentResolver impl, and ModulePolicy. Algorithm constants (max_module_size, max_lookback_gap_secs, max_ancestor_hops, collapse_adjacent_same_root) are exposed as Spec.policy knobs. Adjacent modules sharing the same rootId collapse into a single module, naturally grouping all top-level podcast comments on the same External URI episode into one module. [^423f3-14]


The Grouper tracks supersession via a `superseded_by: BTreeMap<target, BTreeSet<superseder>>`, evicting the target's standalone block when a superseder arrives, suppressing late-arriving originals, and restoring the target if the superseder is removed. Reply chains containing a superseded target event are left intact and not evicted when a superseder arrives. [^6e6bc-3]
## Missing Ancestors

Missing ancestors are handled via orphan-buffer hydration (declaring them as ViewDependencies.ids and re-stitching on arrival) rather than extending ViewContext with a read-only store handle. [^423f3-15]

## Walk Algorithm

The walk_chain algorithm requires an orphaned HashSet to stop greedy absorption of events that are themselves buffered awaiting their own parent, and a root_id_mismatched check for correct has_gap on splice paths. [^423f3-16]

## Rejected Alternatives

Option 1 (DynViewModule type-erased registry) is rejected because it violates ADR-0010 which explicitly chose generated app enum over type-erased registry. [^423f3-17]

## Codebase Hygiene

grouper.rs must be split into state.rs, walk.rs, splice.rs, and delta.rs when next touched since it sits at 499 LOC with zero hard-cap headroom. [^423f3-18]
## See Also

