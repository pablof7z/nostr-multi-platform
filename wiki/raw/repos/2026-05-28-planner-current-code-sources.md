---
title: "Planner Current Code Sources 2026-05-28"
summary: "Source notes for nmp-planner, LogicalInterest, CompiledPlan, routing source lanes, and current code-vs-guide lookup paths."
tags: [repo, planner, routing, subscriptions]
source_type: repo-snapshot
repo: /Users/pablofernandez/Work/nostr-multi-platform
commit: 50ecae23b3587affa1ae167baa067a1e07b9a677
ingested: 2026-05-28
updated: 2026-05-28
---

# Planner Current Code Sources 2026-05-28

## Primary Source Files

- `crates/nmp-planner/src/lib.rs`
- `crates/nmp-planner/src/interest.rs`
- `crates/nmp-planner/src/plan.rs`
- `crates/nmp-planner/src/compiler/mod.rs`
- `crates/nmp-planner/src/compiler/partition/`
- `crates/nmp-planner/src/lattice/`
- `docs/builder-guide/07-subscription-planner.md`
- `docs/design/subscription-compilation/`
- `docs/architecture/crate-boundaries.md`

## Crate Location

The current implementation lives in `crates/nmp-planner`. The crate-level docs
say it was extracted from `nmp-core::planner` during crate-boundary step 9 and
that `nmp-core` re-exports its public surface for existing call sites.

Builder-guide prose may still mention the older `crates/nmp-core/src/planner`
path. For code lookup, use `crates/nmp-planner`.

## Public Surface

The public surface exports:

- `LogicalInterest`, `InterestShape`, `InterestScope`, `InterestLifecycle`
- `RelayHint`, `HintSource`, `PTagRouting`, and `NaddrCoord`
- `SubscriptionCompiler`, `CompileContext`, `MailboxCache`, and mailbox snapshots
- `CompiledPlan`, `RelayPlan`, `SubShape`, `RoutingSource`
- `merge` and `MergeOutcome` for the audit gate
- relay-score lookup helpers used by selection

## LogicalInterest Shape

`InterestShape` mirrors Nostr filter fields with deterministic sorted
containers, then adds client-side routing metadata:

- authors, kinds, tags, since/until, limit, event ids
- address coordinates for parameterized replaceable events
- `relay_pin` for host-bound subscriptions
- `p_tag_routing` for choosing public NIP-65 read relays versus NIP-17 DM relays

`relay_pin` is not serialized into the wire filter. It is routing metadata.

## Compiler Pipeline

`SubscriptionCompiler` compiles interests into a `CompiledPlan` in four stages:

1. partition each interest into relay entries using mailbox, app-relay, indexer,
   and bootstrap relay context;
2. group entries by relay URL;
3. merge compatible shapes through the lattice;
4. compute a stable plan id from sorted inputs, referenced mailbox facts, and
   lattice version.

The compiler has constructors for static tests, active-account read relays,
full relay context, and bootstrap relay context.

## Plan Output

`CompiledPlan` contains a stable `plan_id`, a per-relay map of `RelayPlan`s,
and derived `unroutable_authors`. `RelayPlan.role_tags` can carry multiple
`RoutingSource`s so diagnostics preserve why a relay was selected.

Indexer fallback is represented as `UserConfigured(Indexer)`, not as its own
diagnostic lane.

## Authority Notes

Use design docs for intended semantics and code for current field names,
module paths, and emitted data. The wiki should not preserve migration-step
history as architecture; the durable boundary is that planner implementation
lives outside `nmp-core` and the kernel consumes it through substrate seams.
